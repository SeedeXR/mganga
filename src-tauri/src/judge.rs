// Brick 3: the judgment engine. Offline, no network.
//
// Verdict order of authority:
//   1. The protected list (in code, non-negotiable). These break the session
//      or boot if touched. Brick 4 will enforce this same list in the broker.
//   2. The known-apps list (known_apps.json, plain data, easy to extend).
//      First matching rule wins, so specific rules go before broad ones.
//   3. Heuristics for everything unknown: suspicious paths, missing
//      publisher, and cautious defaults per kind.
//
// Verdicts: "safe-to-disable" | "keep" | "your-call" | "protected".
// Every verdict carries a reason, one human sentence. If Mganga cannot say
// why, it does not make the claim, so the fallbacks are honest about not
// knowing.

use crate::autostart::AutostartEntry;
use crate::evidence::FileEvidence;
use serde::Deserialize;
use std::sync::OnceLock;

/// Services that keep the session or boot alive. Disabling any of these can
/// leave Windows unable to log on, reach the network, or defend itself.
/// Matched against the service's registry key name, lowercase.
const PROTECTED_SERVICES: &[&str] = &[
    "rpcss", "rpceptmapper", "dcomlaunch", "lsm", "plugplay", "power",
    "winmgmt", "eventlog", "schedule", "profsvc", "usermanager", "cryptsvc",
    "bfe", "mpssvc", "windefend", "securityhealthservice", "wscsvc",
    "dnscache", "nsi", "brokerinfrastructure", "coremessagingregistrar",
    "systemeventsbroker", "samss", "keyiso", "gpsvc", "eventsystem",
    "staterepository", "audiosrv", "audioendpointbuilder", "lanmanworkstation",
    "wlansvc", "netman", "nlasvc", "dhcp", "winhttpautoproxysvc", "lsm",
];

#[derive(Deserialize)]
struct Rule {
    #[serde(default)]
    exe: Vec<String>,
    #[serde(default)]
    name_contains: Vec<String>,
    #[serde(default)]
    publisher_contains: Vec<String>,
    /// Restrict the rule to one kind ("run" | "folder" | "task" | "service").
    #[serde(default)]
    kind: Option<String>,
    category: String,
    verdict: String,
    reason: String,
}

static RULES: OnceLock<Vec<Rule>> = OnceLock::new();

fn rules() -> &'static [Rule] {
    RULES.get_or_init(|| {
        serde_json::from_str(include_str!("known_apps.json"))
            .expect("known_apps.json must be valid JSON")
    })
}

pub struct Judgment {
    pub verdict: String,
    pub reason: String,
    pub category: Option<String>,
}

pub fn judge(
    entry: &AutostartEntry,
    exe_path: Option<&str>,
    evidence: Option<&FileEvidence>,
) -> Judgment {
    let name = entry.name.to_lowercase();
    let publisher = entry.publisher.as_deref().unwrap_or("").to_lowercase();
    let exe_name = exe_path
        .and_then(|p| std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    // 1. Protected services, by registry key name.
    if entry.kind == "service" {
        let key_name = entry
            .source_detail
            .rsplit('\\')
            .next()
            .unwrap_or("")
            .to_lowercase();
        // Per-user service instances get a suffix like cbdhsvc_4aeaa.
        let base = key_name.split('_').next().unwrap_or(&key_name);
        if PROTECTED_SERVICES.contains(&key_name.as_str())
            || PROTECTED_SERVICES.contains(&base)
        {
            return Judgment {
                verdict: "protected".into(),
                reason: "This keeps Windows running. Mganga will not touch it.".into(),
                category: Some("windows-core".into()),
            };
        }
    }

    // 2. Known-apps list, first match wins. 3. Heuristics for unknowns.
    let known = rules().iter().find(|rule| {
        if let Some(k) = &rule.kind {
            if *k != entry.kind {
                return false;
            }
        }
        let exe_hit = rule.exe.iter().any(|e| e == &exe_name);
        let name_hit = rule.name_contains.iter().any(|n| name.contains(n));
        let pub_hit = !publisher.is_empty()
            && rule.publisher_contains.iter().any(|p| publisher.contains(p));
        exe_hit || name_hit || pub_hit
    });
    let mut judgment = match known {
        Some(rule) => Judgment {
            verdict: rule.verdict.clone(),
            reason: rule.reason.clone(),
            category: Some(rule.category.clone()),
        },
        None => heuristic(entry, exe_path, &publisher, evidence),
    };

    // 4. Usage evidence (UserAssist). Sharpen the verdict, never override
    // protected or keep. No record means no claim.
    apply_usage(entry, &mut judgment);
    judgment
}

/// Android-style usage evidence: if the user has not deliberately opened an
/// app in a long time, its autostart is probably not earning its place. If
/// they use it constantly, say so honestly.
fn apply_usage(entry: &AutostartEntry, judgment: &mut Judgment) {
    if judgment.verdict == "protected" || judgment.verdict == "keep" {
        return;
    }
    // Services do not appear in UserAssist; a stray match would be noise.
    if entry.kind == "service" {
        return;
    }
    // The last-run timestamp is the reliable signal. The run counter is
    // often stuck at 0 on Windows 11, so it only ever strengthens, never gates.
    let Some(days) = entry.last_opened_days else {
        return;
    };
    let count = entry.open_count.unwrap_or(0);

    if days >= 180 {
        if judgment.verdict == "your-call" {
            judgment.verdict = "safe-to-disable".into();
        }
        judgment.reason.push_str(&format!(
            " You haven't opened it in about {}, so it is probably not earning its place at startup.",
            humanize_days(days)
        ));
    } else if days >= 60 {
        judgment.reason.push_str(&format!(
            " You last opened it about {} ago.",
            humanize_days(days)
        ));
    } else if days <= 7 && count >= 20 {
        judgment
            .reason
            .push_str(" You use this often, so keeping it ready may genuinely save you time.");
    }
}

/// Verdict for a RUNNING process: what stopping it right now would cost.
/// Different question from the autostart verdict, so different wording.
/// Returns None when Mganga does not know the process: no claim, no tag.
pub fn judge_process(name: &str) -> Option<(String, String)> {
    // Windows helpers that respawn on their own: stopping them gains nothing.
    const SELF_RESTARTING: &[&str] = &[
        "runtimebroker", "searchhost", "startmenuexperiencehost",
        "shellexperiencehost", "textinputhost", "widgets", "widgetservice",
        "backgroundtaskhost", "dllhost", "taskhostw", "wmiprvse",
        "searchindexer", "applicationframehost", "systemsettings",
    ];
    if SELF_RESTARTING.contains(&name) {
        return Some((
            "keep".into(),
            "Part of Windows. It restarts itself, so stopping it gains nothing.".into(),
        ));
    }

    const BROWSERS: &[&str] = &["chrome", "msedge", "firefox", "brave", "opera", "vivaldi"];
    if BROWSERS.contains(&name) {
        return Some((
            "your-call".into(),
            "Your browser. Stopping it closes every tab; unsaved work in web apps is lost.".into(),
        ));
    }
    if name == "msedgewebview2" {
        return Some((
            "your-call".into(),
            "A browser engine other apps embed for their windows. Stopping it can blank those apps until they restart.".into(),
        ));
    }

    // Reuse the known-apps list, matched by exe name, but translate each
    // category into what stopping the RUNNING process costs.
    let exe = format!("{name}.exe");
    let rule = rules().iter().find(|r| {
        r.exe.iter().any(|e| e == &exe) || r.name_contains.iter().any(|n| name.contains(n))
    })?;
    let (verdict, reason) = match rule.category.as_str() {
        "updater" | "printer-utility" => (
            "fine-to-stop",
            "A background helper. Stopping it loses nothing; it runs again when it is needed.",
        ),
        "game-launcher" => (
            "fine-to-stop",
            "A launcher idling in the background. Stopping it loses nothing.",
        ),
        "media" => (
            "fine-to-stop",
            "Stopping it just closes the app. Nothing is lost.",
        ),
        "sync-client" => (
            "your-call",
            "Stopping it pauses file syncing until you open it again. No files are lost.",
        ),
        "chat" | "meetings" => (
            "your-call",
            "Stopping it means messages and calls stop arriving until you open it again.",
        ),
        "remote-access" => (
            "your-call",
            "Stopping it cuts off remote access to this PC until it runs again.",
        ),
        "vendor-utility" | "rgb-utility" => (
            "your-call",
            "A vendor helper. Extras like lighting or special keys pause until it runs again.",
        ),
        "download-manager" => (
            "your-call",
            "Stopping it means downloads are no longer caught until you open it again.",
        ),
        "torrent" => (
            "your-call",
            "Stopping it stops your downloads and seeding until you open it again.",
        ),
        "dev-tool" | "phone-link" => (
            "your-call",
            "Stopping it pauses what it does until you open it again.",
        ),
        "audio-driver" | "graphics-driver" => (
            "keep",
            "Part of how your audio or graphics works right now. Stopping it can break things until a restart.",
        ),
        "windows-security" | "antivirus" | "windows-plumbing" => (
            "keep",
            "Part of Windows' own plumbing or protection. Best left alone.",
        ),
        _ => return None,
    };
    Some((verdict.to_string(), reason.to_string()))
}

fn humanize_days(days: u32) -> String {
    match days {
        0..=13 => format!("{days} days"),
        14..=59 => format!("{} weeks", days / 7),
        60..=364 => format!("{} months", days / 30),
        365..=729 => "a year".to_string(),
        _ => format!("{} years", days / 365),
    }
}

fn heuristic(
    entry: &AutostartEntry,
    exe_path: Option<&str>,
    publisher: &str,
    evidence: Option<&FileEvidence>,
) -> Judgment {
    let is_microsoft = publisher.contains("microsoft");
    let path_lower = exe_path.unwrap_or("").to_lowercase();

    // A program's own FileDescription is often the clearest hint about what it
    // is. Fold it into any unknown verdict where Windows recorded one, so the
    // text is specific instead of generic.
    let describe = |reason: String| -> String {
        match evidence.and_then(|e| e.description.as_deref()) {
            Some(d) => format!("{reason} It describes itself as \"{d}\"."),
            None => reason,
        }
    };

    // Things launching from temp folders deserve a hard look.
    if path_lower.contains(r"\temp\") || path_lower.contains(r"\tmp\") {
        return Judgment {
            verdict: "your-call".into(),
            reason: describe("This starts from a temporary folder, which honest apps rarely do. If the name means nothing to you, turning it off is reasonable.".into()),
            category: Some("suspicious-path".into()),
        };
    }

    // RunOnce entries are one-shot installer leftovers, not recurring cost.
    if entry.source.contains("RunOnce") {
        return Judgment {
            verdict: "keep".into(),
            reason: "A one-time step an installer left behind. It runs once at the next start and then removes itself.".into(),
            category: Some("run-once".into()),
        };
    }

    match entry.kind.as_str() {
        "service" => {
            if is_microsoft {
                Judgment {
                    verdict: "keep".into(),
                    reason: "A Windows service that ships with the system. Services are easy to break, so leaving it on is the safe choice.".into(),
                    category: Some("windows-service".into()),
                }
            } else {
                Judgment {
                    verdict: "your-call".into(),
                    reason: describe("A background service installed by one of your apps. Mganga does not know it, so be careful: if that app misbehaves after you stop this, it needed it.".into()),
                    category: Some("third-party-service".into()),
                }
            }
        }
        "task" => {
            if is_microsoft {
                Judgment {
                    verdict: "keep".into(),
                    reason: "Windows housekeeping. It runs briefly at logon and stays out of the way.".into(),
                    category: Some("windows-task".into()),
                }
            } else {
                Judgment {
                    verdict: "your-call".into(),
                    reason: describe("One of your apps scheduled this to run at logon, usually for updates or telemetry. The app itself keeps working without it.".into()),
                    category: Some("third-party-task".into()),
                }
            }
        }
        _ => {
            if publisher.is_empty() {
                Judgment {
                    verdict: "your-call".into(),
                    reason: describe("It does not say who made it. If the name means nothing to you, try turning it off, you can always turn it back on.".into()),
                    category: Some("unknown".into()),
                }
            } else {
                Judgment {
                    verdict: "your-call".into(),
                    reason: describe("Mganga does not know this app. It starts with Windows, but most apps work just as well started by hand.".into()),
                    category: Some("unknown".into()),
                }
            }
        }
    }
}
