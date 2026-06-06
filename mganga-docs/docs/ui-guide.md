# UI guide

## Who this is for

Someone who knows a bit about computers. They understand "process," "RAM," "startup
program." They do **not** know "ASEP," "EcoQoS," "WOW6432Node," or registry hives. Mganga
never makes them feel dumb and never assumes they are an expert. The job is to make a
capable-but-not-specialist person feel in control and informed.

## The one principle: always say the why

Mganga's whole reason to exist over Task Manager is that it explains. Every verdict, every
suggestion, every locked item carries a short, true, plain-language reason. If Mganga
cannot say why, it does not make the claim.

Two mechanisms:
- **Inline reason text.** Short sentence next to the thing itself. Always visible, not
  hidden behind a hover, for the primary verdict.
- **Hint explainers.** A small "?" or info affordance for the deeper or more technical bit
  (what "Efficiency mode" actually does, what "suspend" means). One or two sentences on
  tap/hover. This is where the jargon gets translated, so the main view stays clean.

Never show a technical term naked. Either translate it inline or attach a hint.

## The screens

Keep it to one window, a few clear views. Suggested:

**1. Right now (the diagnosis).** Lands here. A plain-language summary at the top:
"Your machine is using 81% of its memory. The three biggest reasons are X, Y, Z." Then the
live process view (Brick 5) underneath, sorted by cost, with "why heavy" hints. This screen
answers the actual question the user walked in with: why is it slow right now.

**2. Startup (the vetting).** The autostarter inventory (Brick 2/3) as a list. Each row:
- name + small icon + publisher
- current state (on / off) as a clear toggle
- the **verdict** as a colored tag
- the **reason** as one line under the name
Group by verdict or let the user filter ("show me what's safe to turn off"). A one-line
header summary: "8 of these launch at boot. 3 of them probably don't need to."

**3. Processes / control.** Reachable from a heavy process in screen 1, or its own tab.
Per-process actions in the gentle-first order (see below).

(An audit-log / history view can be a small fourth screen or a drawer.)

## Verdict styling

Four verdicts, each a calm color and a plain word. Avoid alarm-red except for genuine risk.

- **Safe to turn off** — muted green/neutral. "This is a printer updater. It only runs to
  check for updates, it doesn't need to start with Windows."
- **Your call** — amber. "This syncs your files. Turn it off if you don't need files synced
  the moment you log in; you can always start it yourself."
- **Keep** — neutral. "This is part of how your audio works. Leaving it on is the safe choice."
- **Protected** — locked, with a small lock icon. "This keeps Windows running. Mganga won't
  touch it." Not toggleable.

The reason text is the product. Write each one as a knowledgeable friend would say it out
loud. True, specific, no hedging, no jargon.

## Gentle before violent

On the process control screen, present actions in this order, with the gentle one visually
primary and kill visually de-emphasized:

1. **Ease off (Efficiency mode)** — primary. Hint: "Tells Windows to run this slowly on its
   efficient cores so it stops hogging power. It keeps working, just quietly. Reversible."
2. **Pause (Suspend)** — secondary. Hint: "Freezes the app where it is so it uses no CPU.
   Hit resume to wake it exactly where it left off."
3. **Stop (Kill)** — tertiary, behind a confirm. Hint: "Force-closes it. Any unsaved work in
   it is lost."

The point Einstein cares about: do not lead with the kill button. A healer throttles before
he amputates.

## Protected items and scary confirms

- **Protected items** appear in the lists (transparency, the user sees the full picture) but
  are visibly locked with the one-line why. They are never actionable in the UI, and the
  broker would refuse anyway.
- **Scary confirm** for kill, for stopping a service, and for any irreversible action. State
  the plain consequence, not a generic "Are you sure?": "Stop this service? Your printer may
  stop working until you start it again or restart the PC." Require an explicit click.

## Microcopy voice

- Friendly, direct, plain. Like a sharp friend who knows computers explaining over your
  shoulder.
- Short. One sentence beats three.
- No em-dashes. Use commas, periods, or restructure.
- No fear-mongering, no marketing. State what is true and what will happen.
- The app's name is Swahili; the interface language is English. Keep it warm, not corporate.
