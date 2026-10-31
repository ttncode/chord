//! Finds clickable elements in a window with UI Automation (patterns from mousemaster / hunt-and-peck):
//! one cached FindAll over the window, then drop elements another window covers.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_None, CUIAutomation, IUIAutomation, IUIAutomationCondition, TreeScope_Children,
    TreeScope_Descendants, UIA_BoundingRectanglePropertyId, UIA_IsEnabledPropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId, UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsTogglePatternAvailablePropertyId, UIA_PROPERTY_ID,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};
use windows::core::{Result, VARIANT};

/// More labels than this are unreadable; mousemaster caps similarly.
const MAX_ELEMENTS: i32 = 300;
/// Element centres closer than this (pixels) get one label.
const MIN_GAP: i32 = 6;

/// Centres (physical screen pixels) of clickable, visible elements in `hwnd`, top-left first.
pub fn clickable_points(hwnd: HWND) -> Result<Vec<POINT>> {
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
        let condition = uia.CreateAndCondition(&visible, &clickable)?;

        let found = root.FindAllBuildCache(TreeScope_Descendants, &condition, &cache)?;
        let target_root = GetAncestor(hwnd, GA_ROOT);
        let mut points: Vec<POINT> = Vec::new();
        for i in 0..found.Length()?.min(MAX_ELEMENTS) {
            let Ok(rect) = found.GetElement(i).and_then(|e| e.CachedBoundingRectangle()) else { continue };
            if let Some(center) = center(rect) {
                // Covered by another window (menus, overlays, other apps): not clickable here.
                if GetAncestor(WindowFromPoint(center), GA_ROOT) != target_root {
                    continue;
                }
                if points.iter().all(|p| (p.x - center.x).abs() > MIN_GAP || (p.y - center.y).abs() > MIN_GAP) {
                    points.push(center);
                }
            }
        }
        points.sort_by_key(|p| (p.y, p.x));
        Ok(points)
    }
}

fn center(r: RECT) -> Option<POINT> {
    (r.right > r.left && r.bottom > r.top).then(|| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 })
}
