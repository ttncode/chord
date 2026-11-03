# Flow: hint labels

Entrypoint: `src/win.rs:418`

```mermaid
sequenceDiagram
  participant L as effect loop (win.rs)
  participant P as panel.rs
  participant U as uia.rs
  participant A as App (UI Automation)
  participant T as hints.rs

  L->>P: hide_now (panel must not cover targets)
  L->>U: clickable_rects(foreground window)
  U->>A: WM_GETOBJECT to Chrome_RenderWidgetHostHWND (wake)
  U->>A: FindAll dialogs, pick focused / uncovered one
  U->>A: FindAllBuildCache(clickable, not container)
  A-->>U: elements: rect, control type, name
  U->>A: empty? read the page window directly
  U-->>L: rects + is-real-control
  L->>T: targets() then labels()
  L->>P: show_hints(area, labels, typed)
  L->>L: letters filter labels, Space up clicks the centre
```

| # | Step | Where |
| - | ---- | ----- |
| 1 | Typing exactly a hint chord (`;`, `'`) opens labels; Caps repeat and Shift reopen them after a click | `src/win.rs:331`, `src/win.rs:361`, `src/config.rs:204` |
| 2 | The panel is hidden synchronously before the scan | `src/win.rs:418`, `src/panel.rs:162` |
| 3 | Up to 3 reads (0, 300, 600 ms) while Electron rebuilds its tree | `src/win.rs:198` |
| 4 | Chromium page windows get a wake-up `WM_GETOBJECT` | `src/uia.rs:319` |
| 5 | An open modal limits the scan: the focused one first, then one not covered at its centre | `src/uia.rs:152`, `src/uia.rs:361` |
| 6 | Elements whose centre is covered by another app's window are dropped | `src/uia.rs:123` |
| 7 | An empty read falls back to the page window (Electron after a minimize) | `src/uia.rs:187` |
| 8 | Parts of real controls and generic boxes around other clickables lose their label, except items nested in an open tree/list item (which is cut to its own row); cap 300 after filtering | `src/hints.rs:40` |
| 8b | `hints = "scroll"` reads scroll areas instead (scroll pattern, scrollable; plus navigation landmarks, as Chromium can leave a sidebar's scroll box out of the tree); only areas ≥ 80 px a side showing < 95 % of their content (Chromium calls 1 px of overflow scrollable); nested ones all keep a label; none found falls back to the clickable read. A pick only moves the pointer | `src/uia.rs:230`, `src/hints.rs:76` |
| 9 | Prefix-free labels from `sadfjklewcmpgh` | `src/hints.rs:92` |
| 10 | Labels drawn at each element's top-left on a click-through overlay | `src/panel.rs:574` |
| 11 | Space up with a full label moves the pointer to the element centre and clicks | `src/win.rs:437`, `src/win.rs:254` |

## Failure modes

| When | Result | Handled at |
| --- | --- | --- |
| UI Automation errors (e.g. `0x80040201` in Chrome) | `could not read this window's buttons`, after 3 tries | `src/win.rs:198` |
| Nothing clickable reported | `no clickable elements found`; Space up says `no labels on screen to pick from` | `src/win.rs:198`, `src/win.rs:393` |
| Element exists only as pixels (custom-drawn, Postman's ☰ and window buttons) | no label; nothing to read | — |
| A closed dialog lingers in the tree | ignored unless something in it is clickable | `src/uia.rs:152` |
