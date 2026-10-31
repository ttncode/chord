//! Finds clickable elements in a window with UI Automation (patterns from mousemaster / hunt-and-peck):
//! one cached FindAll over the window, then drop elements another window covers.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_None, CUIAutomation, IUIAutomation, IUIAutomationCondition, TreeScope_Children,
    TreeScope_Descendants, UIA_BoundingRectanglePropertyId, UIA_IsEnabledPropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId, UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsDialogPropertyId, UIA_IsTogglePatternAvailablePropertyId, UIA_PROPERTY_ID, UIA_ControlTypePropertyId, UIA_DataGridControlTypeId,
    UIA_DocumentControlTypeId, UIA_GroupControlTypeId, UIA_HeaderControlTypeId, UIA_ListControlTypeId,
    UIA_MenuBarControlTypeId, UIA_PaneControlTypeId, UIA_ScrollBarControlTypeId, UIA_StatusBarControlTypeId,
    UIA_TabControlTypeId, UIA_TableControlTypeId, UIA_TitleBarControlTypeId, UIA_ToolBarControlTypeId,
    UIA_TreeControlTypeId, UIA_WindowControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};
use windows::core::{Result, VARIANT};

use crate::hints::Rect;

/// Rectangles (physical screen pixels) of clickable elements in `hwnd` that no other window covers.
pub fn clickable_rects(hwnd: HWND) -> Result<Vec<Rect>> {
    unsafe {
        // Safe to repeat: later calls on this thread just return S_FALSE.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let root = uia.ElementFromHandle(hwnd)?;
        // Chromium/Electron build their accessibility tree lazily on the first query; this wakes it.
        let _ = root.FindAll(TreeScope_Children, &uia.CreateTrueCondition()?);

        let cache = uia.CreateCacheRequest()?;
        cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
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

        // An open modal (a web dialog, or one an app draws inside its window): only its contents count.
        let open_dialog = uia.CreateAndCondition(&is(UIA_IsDialogPropertyId, true)?, &is(UIA_IsOffscreenPropertyId, false)?)?;
        let dialogs = root.FindAll(TreeScope_Descendants, &open_dialog)?;
        let scope = match dialogs.Length()? {
            0 => root,
            // The last one in the tree is the one on top.
            n => dialogs.GetElement(n - 1)?,
        };
        let found = scope.FindAllBuildCache(TreeScope_Descendants, &condition, &cache)?;
        let target_root = GetAncestor(hwnd, GA_ROOT);
        let mut rects: Vec<Rect> = Vec::new();
        for i in 0..found.Length()? {
            let Ok(rect) = found.GetElement(i).and_then(|e| e.CachedBoundingRectangle()) else { continue };
            // Covered by another window (menus, overlays, other apps): not clickable here.
            if let Some(center) = center(rect) {
                if GetAncestor(WindowFromPoint(center), GA_ROOT) == target_root {
                    rects.push((rect.left, rect.top, rect.right, rect.bottom));
                }
            }
        }
        Ok(rects)
    }
}

fn center(r: RECT) -> Option<POINT> {
    (r.right > r.left && r.bottom > r.top).then(|| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 })
}
