# Brick 9: the connection check

Status: **spec only, not approved, no code written.**
Written 2026-08-30, after the owner asked whether Mganga could discover blocked
services on its own.

## The question this answers

"This site will not load. Is it me, the site, or my provider?"

Nobody can answer that today without either a technical friend or trial and
error with a VPN. It is the same shape as the two questions Mganga already
answers: something is wrong with the machine, and no honest explanation is
available in the box.

## What was asked, and what changed

The ask was: on install, scan the device for services being blocked by the ISP,
then pop a card on Home offering the fix.

**The scan half cannot be built honestly.** Nothing on a Windows machine records
"my provider dropped this connection". There is no event log entry, no counter,
no registry key. A dropped connection and a website that is down are
indistinguishable from the outside. Reading individual apps' own logs would mean
hardcoding a list of apps, which is exactly the assumption the brick 8
generalization removed. To know a block exists, something has to open a
connection and watch it fail. That is network, and the standing rule is that
Mganga asks before it uses the network.

**So the trigger moves from the scanner to the user.** Same outcome, honest
mechanism: Mganga asks once, the user answers once, and from then on the check
runs only when they ask for it.

## What the check actually measures

The four causes of "it will not load" leave different fingerprints, and they are
separable in about a second per site. This is a measurement, not a guess.

| Step | Observation | Conclusion |
|---|---|---|
| 1 | Name does not resolve, or resolves somewhere wrong | Blocked at the name lookup (DNS) |
| 2 | Name resolves, TCP connect to :443 never completes | Blocked by address |
| 3 | TCP connects, then dies the instant the site name is sent, **and the same address stays alive when no name is sent** | Filtered by name, the case an unblocker fixes |
| 4 | Connection completes, the site answers with a refusal | The site itself is refusing, not the provider |

Step 3 is the load-bearing one. The contrast between "dies with the name" and
"survives without the name" is what makes this a diagnosis rather than a hunch,
and it is the only one of the four that an SNI unblocker can do anything about.

## The rules this brick must not break

1. **Ask before the network, every time.** First run asks once, plainly: "Want
   Mganga to check whether anything is being blocked by your provider? This is
   the only part of Mganga that uses the internet." Yes, or never. A "never"
   answer is remembered and never asked again.
2. **The user picks the site.** Mganga does not carry a list of sites people
   might want. The user types what is broken. A machine that already has an
   unblocker installed may offer that tool's own blacklist as suggestions,
   because that list was read from the machine, not shipped.
3. **Explain, never install.** If the check finds name-based filtering, Mganga
   says so, says a class of free tool addresses this exact case without a VPN,
   links to the project page, and notes such tools are restricted in some
   countries. Mganga does not download, install, or configure anything. Reasons:
   the legal exposure is the user's to accept knowingly; GoodbyeDPI ships a
   kernel packet driver (WinDivert) that the spec already ruled out
   distributing; and shipping AV-flagged binaries would burn the code-signing
   reputation being built in issue #4.
4. **Report only what was measured.** Four outcomes plus "inconclusive". No
   fifth outcome invented to sound helpful.

## Scope

In: one Rust module, a user-triggered command taking a hostname, the four-step
probe, a result panel, and the one-time consent question stored with the other
settings.

Out: background scanning, scheduled re-checks, telemetry of any kind, any
site list shipped with the app, and anything that writes to the machine.

## The gate

With the unblocker service stopped, check a site on its blacklist: the result
must read "filtered by name". Start the service, check again: must read "gets
through". Check a site that is genuinely down: must not say "filtered by name".
Check a normal site: must say nothing is wrong. Then decline the consent
question on a fresh profile and confirm no packet leaves the machine.
