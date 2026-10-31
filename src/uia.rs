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
    UIA_DocumentControlTypeId, UIA_HeaderControlTypeId, UIA_ListControlTypeId,
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
        // Groups are not on the list: web apps (Postman) report their buttons as groups, and a group
        // around other clickable things gives way to them anyway (see `hints::targets`).
        let mut container = uia.CreateFalseCondition()?;
        for kind in [
            UIA_ListControlTypeId,
            UIA_TreeControlTypeId,
            UIA_PaneControlTypeId,
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
        // The one on top holds the keyboard focus (modals trap it); after it, later in the tree is
        // usually higher. Closed ones can linger in the tree empty, so a dialog wins only when
        // something in it is clickable.
        let open_dialog = uia.CreateAndCondition(&is(UIA_IsDialogPropertyId, true)?, &is(UIA_IsOffscreenPropertyId, false)?)?;
        let found_dialogs = root.FindAll(TreeScope_Descendants, &open_dialog)?;
        let mut dialogs: Vec<IUIAutomationElement> =
            (0..found_dialogs.Length()?).rev().filter_map(|i| found_dialogs.GetElement(i).ok()).collect();
        if let Some(focused) = focused_dialog(&uia) {
            dialogs.insert(0, focused);
        }
        let mut result = None;
        for dialog in &dialogs {
            let rects = rects_in(dialog)?;
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
            log += &format!("\n{} dialog(s) open\n\nEVERY ELEMENT (offscreen, enabled, focusable, invokable):\n", dialogs.len());
            log += &every_element(&uia, &root).unwrap_or_else(|e| format!("could not list: {e}\n"));
            let _ = std::fs::write("hints-debug.txt", log);
            say!("  (debug: wrote hints-debug.txt)");
        }
        Ok(result)
    }
}

/// The dialog holding the keyboard focus, if any: walks up from the focused element.
fn focused_dialog(uia: &IUIAutomation) -> Option<IUIAutomationElement> {
    unsafe {
        let walker = uia.RawViewWalker().ok()?;
        let mut element = uia.GetFocusedElement().ok()?;
        for _ in 0..64 {
            let is_dialog = element.GetCurrentPropertyValue(UIA_IsDialogPropertyId).ok();
            if is_dialog.is_some_and(|v| bool::try_from(&v).unwrap_or(false)) {
                return Some(element);
            }
            element = walker.GetParentElement(&element).ok()?;
        }
        None
    }
}

/// Debug dump of every element under `root`, clickable or not, to see why a button got no label.
fn every_element(uia: &IUIAutomation, root: &IUIAutomationElement) -> Result<String> {
    unsafe {
        let cache = uia.CreateCacheRequest()?;
        for id in [
            UIA_BoundingRectanglePropertyId,
            UIA_ControlTypePropertyId,
            UIA_NamePropertyId,
            UIA_IsOffscreenPropertyId,
            UIA_IsEnabledPropertyId,
            UIA_IsKeyboardFocusablePropertyId,
            UIA_IsInvokePatternAvailablePropertyId,
        ] {
            cache.AddProperty(id)?;
        }
        cache.SetAutomationElementMode(AutomationElementMode_None)?;
        let all = root.FindAllBuildCache(TreeScope_Descendants, &uia.CreateTrueCondition()?, &cache)?;
        let flag = |e: &IUIAutomationElement, id| {
            e.GetCachedPropertyValue(id).ok().and_then(|v| bool::try_from(&v).ok()).map_or('?', |b| if b { 'y' } else { '-' })
        };
        let mut out = String::new();
        for i in 0..all.Length()? {
            let Ok(e) = all.GetElement(i) else { continue };
            let r = e.CachedBoundingRectangle().unwrap_or_default();
            out += &format!(
                "type {:5}  {}{}{}{}  {:?}  {:?}\n",
                e.CachedControlType().unwrap_or_default().0,
                flag(&e, UIA_IsOffscreenPropertyId),
                flag(&e, UIA_IsEnabledPropertyId),
                flag(&e, UIA_IsKeyboardFocusablePropertyId),
                flag(&e, UIA_IsInvokePatternAvailablePropertyId),
                (r.left, r.top, r.right, r.bottom),
                e.CachedName().map(|n| n.to_string()).unwrap_or_default(),
            );
        }
        Ok(out)
    }
}

fn center(r: RECT) -> Option<POINT> {
    (r.right > r.left && r.bottom > r.top).then(|| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 })
}
