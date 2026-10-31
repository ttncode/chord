//! Finds clickable elements in a window with UI Automation (patterns from mousemaster / hunt-and-peck):
//! one cached FindAll over the window, then drop elements another window covers.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_None, CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement, TreeScope_Children,
    TreeScope_Descendants, UIA_BoundingRectanglePropertyId, UIA_IsEnabledPropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId, UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsDialogPropertyId, UIA_IsTogglePatternAvailablePropertyId, UIA_PROPERTY_ID, UIA_CONTROLTYPE_ID,
    UIA_ButtonControlTypeId, UIA_SplitButtonControlTypeId, UIA_HyperlinkControlTypeId, UIA_MenuItemControlTypeId,
    UIA_TabItemControlTypeId, UIA_ListItemControlTypeId, UIA_TreeItemControlTypeId, UIA_DataItemControlTypeId,
    UIA_CheckBoxControlTypeId, UIA_RadioButtonControlTypeId, UIA_ComboBoxControlTypeId, UIA_EditControlTypeId,
    UIA_SliderControlTypeId, UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_DataGridControlTypeId,
    UIA_DocumentControlTypeId, UIA_GroupControlTypeId, UIA_HeaderControlTypeId, UIA_ListControlTypeId,
    UIA_MenuBarControlTypeId, UIA_PaneControlTypeId, UIA_ScrollBarControlTypeId, UIA_StatusBarControlTypeId,
    UIA_TabControlTypeId, UIA_TableControlTypeId, UIA_TitleBarControlTypeId, UIA_ToolBarControlTypeId,
    UIA_TreeControlTypeId, UIA_WindowControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};
use windows::core::{Result, VARIANT};

use std::sync::atomic::{AtomicBool, Ordering};

use crate::hints::Rect;

/// Set by `--debug-hints`: every scan writes what UI Automation reported to `hints-debug.txt`.
pub static DEBUG: AtomicBool = AtomicBool::new(false);

/// Real controls; anything else clickable (custom, text, image) is a generic box. See `hints::targets`.
const CONTROLS: [UIA_CONTROLTYPE_ID; 13] = [
    UIA_ButtonControlTypeId,
    UIA_SplitButtonControlTypeId,
    UIA_HyperlinkControlTypeId,
    UIA_MenuItemControlTypeId,
    UIA_TabItemControlTypeId,
    UIA_ListItemControlTypeId,
    UIA_TreeItemControlTypeId,
    UIA_DataItemControlTypeId,
    UIA_CheckBoxControlTypeId,
    UIA_RadioButtonControlTypeId,
    UIA_ComboBoxControlTypeId,
    UIA_EditControlTypeId,
    UIA_SliderControlTypeId,
];

/// Rectangles (physical screen pixels) of clickable elements in `hwnd` that no other window covers,
/// each with whether it is a real control.
pub fn clickable_rects(hwnd: HWND) -> Result<Vec<(Rect, bool)>> {
    unsafe {
        // Safe to repeat: later calls on this thread just return S_FALSE.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let root = uia.ElementFromHandle(hwnd)?;
        // Chromium/Electron build their accessibility tree lazily on the first query; this wakes it.
        let _ = root.FindAll(TreeScope_Children, &uia.CreateTrueCondition()?);

        let cache = uia.CreateCacheRequest()?;
        cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
        cache.AddProperty(UIA_ControlTypePropertyId)?;
        cache.AddProperty(UIA_NamePropertyId)?;
        cache.SetAutomationElementMode(AutomationElementMode_None)?;

        let is = |id: UIA_PROPERTY_ID, value: bool| -> Result<IUIAutomationCondition> {
            uia.CreatePropertyCondition(id, &VARIANT::from(value))
        };
        let mut clickable = is(UIA_IsKeyboardFocusablePropertyId, true)?;
        for id in [
            UIA_IsInvokePatternAvailablePropertyId,
            UIA_IsTogglePatternAvailablePropertyId,
            UIA_IsSelectionItemPatternAvailablePropertyId,
            UIA_IsExpandCollapsePatternAvailablePropertyId,
        ] {
            clickable = uia.CreateOrCondition(&clickable, &is(id, true)?)?;
        }
        let visible = uia.CreateAndCondition(&is(UIA_IsOffscreenPropertyId, false)?, &is(UIA_IsEnabledPropertyId, true)?)?;
        // Containers are often keyboard-focusable too, but nobody means to click a whole list or pane.
        let mut container = uia.CreateFalseCondition()?;
        for kind in [
            UIA_ListControlTypeId,
            UIA_TreeControlTypeId,
            UIA_PaneControlTypeId,
            UIA_GroupControlTypeId,
            UIA_ToolBarControlTypeId,
            UIA_DataGridControlTypeId,
            UIA_TableControlTypeId,
            UIA_HeaderControlTypeId,
            UIA_WindowControlTypeId,
            UIA_DocumentControlTypeId,
            UIA_StatusBarControlTypeId,
            UIA_TitleBarControlTypeId,
            UIA_MenuBarControlTypeId,
            UIA_TabControlTypeId,
            UIA_ScrollBarControlTypeId,
        ] {
            let of_kind = uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(kind.0))?;
            container = uia.CreateOrCondition(&container, &of_kind)?;
        }
        let condition =
            uia.CreateAndCondition(&uia.CreateAndCondition(&visible, &clickable)?, &uia.CreateNotCondition(&container)?)?;

        let target_root = GetAncestor(hwnd, GA_ROOT);
        let debug = DEBUG.load(Ordering::Relaxed);
        let mut log = String::new();
        let mut rects_in = |scope: &IUIAutomationElement| -> Result<Vec<(Rect, bool)>> {
            let found = scope.FindAllBuildCache(TreeScope_Descendants, &condition, &cache)?;
            let mut rects = Vec::new();
            for i in 0..found.Length()? {
                let Ok(element) = found.GetElement(i) else { continue };
                let Ok(rect) = element.CachedBoundingRectangle() else { continue };
                let kind = element.CachedControlType().unwrap_or_default();
                // Covered by another window (menus, overlays, other apps): not clickable here.
                let shown = center(rect).is_some_and(|c| GetAncestor(WindowFromPoint(c), GA_ROOT) == target_root);
                if debug {
                    let name = element.CachedName().map(|n| n.to_string()).unwrap_or_default();
                    let fate = if shown { "" } else { "  [covered or empty]" };
                    let r = (rect.left, rect.top, rect.right, rect.bottom);
                    log += &format!("type {:5}  {r:?}  {name:?}{fate}\n", kind.0);
                }
                if shown {
                    rects.push(((rect.left, rect.top, rect.right, rect.bottom), CONTROLS.contains(&kind)));
                }
            }
            Ok(rects)
        };

        // An open modal (a web dialog, or one an app draws inside its window): only its contents count.
        // Closed ones can linger in the tree empty, so a dialog wins only when something in it is clickable.
        let open_dialog = uia.CreateAndCondition(&is(UIA_IsDialogPropertyId, true)?, &is(UIA_IsOffscreenPropertyId, false)?)?;
        let dialogs = root.FindAll(TreeScope_Descendants, &open_dialog)?;
        // The last one in the tree is the one on top.
        let mut result = None;
        for i in (0..dialogs.Length()?).rev() {
            let rects = rects_in(&dialogs.GetElement(i)?)?;
            if !rects.is_empty() {
                result = Some(rects);
                break;
            }
        }
        let result = match result {
            Some(rects) => rects,
            None => rects_in(&root)?,
        };
        if debug {
            let _ = std::fs::write("hints-debug.txt", log);
            say!("  (debug: wrote hints-debug.txt)");
        }
        Ok(result)
    }
}

fn center(r: RECT) -> Option<POINT> {
    (r.right > r.left && r.bottom > r.top).then(|| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 })
}
