# Risks

Read out of the code and the Windows test runs on 2026-11-01, against commit `4824990`. Ordered by impact.

| # | Risk | Where | Impact | Evidence | Mitigation |
| - | ---- | ----- | ------ | -------- | ---------- |
| 1 | Hook callback too slow or blocked | `src/win.rs:475` | Windows silently removes the hook; all chords stop, keys may lag | Windows' ~1 s `LowLevelHooksTimeout` | callback only decides and `try_send`s; hook reinstalled every 5 min and on unlock/resume (`src/win.rs:454`) |
| 2 | Caps Lock light out of sync | `src/win.rs:520` | light on while Caps Lock is off | the keyboard flips the light below the hook; back-to-back toggles lost the second light command; a crash while Caps is held left it flipped | two tagged toggles 60 ms apart after each chord and at startup |
| 3 | Lost key release (focus change, UAC, crash) | `src/machine.rs:116` | chord mode stuck, letters swallowed | releases not delivered to the hook | only the last-pressed key auto-repeats: assume release after 1.5 s (`STALE_MS`) or 15 s idle (`IDLE_MS`) |
| 4 | Empty string to `DrawTextW` | `src/canvas.rs:123` | access violation, process dies | crash on first panel draw with nothing typed | guard returns early for empty text |
| 5 | Electron apps hide their page from UI Automation after a minimize | `src/uia.rs:178` | `;` finds nothing in Postman/Antigravity | debug dump: only 8 empty panes until reading `Chrome_RenderWidgetHostHWND` directly | page-window fallback; `--debug-hints` for new cases |
| 6 | Apps running as administrator | `src/win.rs:475` | chords do nothing while such a window is in front | Windows does not deliver low-level hook events from higher-integrity windows | documented limit; would need chord to run elevated |

## Checked and benign

| Looked like | Why it is not a risk |
| --- | --- |
| Our own `SendInput` re-entering the hook | every injected event carries `TAG`; the hook skips exactly those (`src/win.rs:55`) |
| Vietnamese Telex turning `dd` into `đ` during a chord | letters are blocked in chord mode, the IME never sees them; verified on Windows |
