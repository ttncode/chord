# Risks

Read out of the code and the Windows test runs on 2026-11-01, against commit `4824990`. Ordered by impact.

| # | Risk | Where | Impact | Evidence | Mitigation |
| - | ---- | ----- | ------ | -------- | ---------- |
| 1 | Hook callback too slow or blocked | `src/win.rs:756` | Windows silently removes the hook; all chords stop, keys may lag | Windows' ~1 s `LowLevelHooksTimeout` | callback only decides and `try_send`s; hook reinstalled every 5 min and on unlock/resume (`src/win.rs:735`) |
| 2 | Caps Lock light out of sync | `src/win.rs:823` | light on while Caps Lock is off | the keyboard flips the light below the hook; back-to-back toggles lost the second light command; a crash while Caps is held left it flipped | two tagged toggles 60 ms apart after each chord and at startup |
| 3 | Lost key release (focus change, UAC, crash) | `src/machine.rs:136` | chord mode stuck, letters swallowed | releases not delivered to the hook | only the last-pressed key auto-repeats: assume release after 1.5 s (`STALE_MS`) or 15 s idle (`IDLE_MS`) |
| 4 | Empty string to `DrawTextW` | `src/canvas.rs:123` | access violation, process dies | crash on first panel draw with nothing typed | guard returns early for empty text |
| 5 | Electron apps hide their page from UI Automation after a minimize | `src/uia.rs:198` | `;` finds nothing in Postman/Antigravity | debug dump: only 8 empty panes until reading `Chrome_RenderWidgetHostHWND` directly | page-window fallback; `--debug-hints` for new cases |
| 6 | Apps running as administrator | `src/win.rs:756` | chords do nothing while such a window is in front | Windows does not deliver low-level hook events from higher-integrity windows | documented limit; would need chord to run elevated |
| 7 | Caps Lock loop with a remote desktop or VM client | `src/win.rs:760` | machine hangs for minutes, CPU 100%, screen flickers on every Caps Lock press | v0.2.0 on a laptop driving another PC through `mstsc`: the client re-injects Caps Lock to sync the light, chord took it as a new press and toggled again; quitting chord on the laptop stopped it | hook leaves injected Caps Lock alone, and all Caps Lock while `mstsc`/`msrdc`/VM windows are in front (`src/config.rs:238`); any other loop trips the breaker: 30 effects in 2 s → hook passes everything, message, exit (`src/machine.rs:411`) |

## Checked and benign

| Looked like | Why it is not a risk |
| --- | --- |
| Our own `SendInput` re-entering the hook | every injected event carries `TAG`; the hook skips exactly those (`src/win.rs:55`) |
| Vietnamese Telex turning `dd` into `đ` during a chord | letters are blocked in chord mode, the IME never sees them; verified on Windows |
