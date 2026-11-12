#![allow(non_upper_case_globals)]
//! UI Automation over the last frame's accessibility tree. Providers are
//! handles (a node id); every call reads the live tree on the UI thread.
//! Invoke, Value and Toggle requests become input events for the next
//! frame, so widgets handle them exactly like clicks and typing.

use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Accessibility::*;
use windows::core::*;

use crate::a11y::{Node as A11yNode, Role, Tree};
use crate::input::A11yAction;

const ROOT: u64 = 0;

#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot,
    IInvokeProvider,
    IValueProvider,
    IToggleProvider
)]
struct Node {
    id: u64,
}

pub fn root() -> IRawElementProviderSimple {
    Node { id: ROOT }.into()
}

fn fragment(id: u64) -> IRawElementProviderFragment {
    Node { id }.into()
}

fn na() -> Error {
    HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32).into()
}

fn with_node<R>(id: u64, f: impl FnOnce(&A11yNode, &super::State) -> R) -> Result<R> {
    super::with_state(|s| s.tree.get(id).map(|n| f(n, s)))
        .flatten()
        .ok_or_else(na)
}

fn control_type(role: Role) -> UIA_CONTROLTYPE_ID {
    match role {
        Role::Window => UIA_WindowControlTypeId,
        Role::Group => UIA_GroupControlTypeId,
        Role::Pane => UIA_PaneControlTypeId,
        Role::Dialog => UIA_WindowControlTypeId,
        Role::Button => UIA_ButtonControlTypeId,
        Role::MenuButton => UIA_SplitButtonControlTypeId,
        Role::CheckBox => UIA_CheckBoxControlTypeId,
        Role::ComboBox => UIA_ComboBoxControlTypeId,
        Role::Edit => UIA_EditControlTypeId,
        Role::Text => UIA_TextControlTypeId,
        Role::Link => UIA_HyperlinkControlTypeId,
        Role::List => UIA_ListControlTypeId,
        Role::ListItem => UIA_ListItemControlTypeId,
        Role::Menu => UIA_MenuControlTypeId,
        Role::MenuItem => UIA_MenuItemControlTypeId,
        Role::Tab => UIA_TabControlTypeId,
        Role::TabItem => UIA_TabItemControlTypeId,
        Role::Tree => UIA_TreeControlTypeId,
        Role::TreeItem => UIA_TreeItemControlTypeId,
        Role::Table => UIA_TableControlTypeId,
        Role::Row => UIA_DataItemControlTypeId,
        Role::Cell => UIA_TextControlTypeId,
        Role::Header => UIA_HeaderItemControlTypeId,
        Role::ToolTip => UIA_ToolTipControlTypeId,
        Role::ProgressBar => UIA_ProgressBarControlTypeId,
        Role::Separator => UIA_SeparatorControlTypeId,
    }
}

fn children(tree: &Tree, id: u64) -> Vec<u64> {
    let parent = (id != ROOT).then_some(id);
    tree.children(parent).map(|n| n.id).collect()
}

fn parent_of(tree: &Tree, id: u64) -> Option<u64> {
    if id == ROOT {
        return None;
    }
    Some(tree.get(id)?.parent.unwrap_or(ROOT))
}

fn act(action: A11yAction) -> Result<()> {
    super::with_state(|s| s.a11y_action(action)).ok_or_else(na)
}

fn runtime_id(id: u64) -> *mut SAFEARRAY {
    let parts = [UiaAppendRuntimeId as i32, (id >> 32) as i32, id as i32];
    unsafe {
        let sa = SafeArrayCreateVector(VT_I4, 0, parts.len() as u32);
        if sa.is_null() {
            return sa;
        }
        for (i, v) in parts.iter().enumerate() {
            let index = i as i32;
            let _ = SafeArrayPutElement(sa, &index, (v as *const i32).cast());
        }
        sa
    }
}

/// Tells screen readers that keyboard focus moved.
pub fn focus_changed(tree: &Tree) {
    unsafe {
        if !UiaClientsAreListening().as_bool() {
            return;
        }
        if let Some(id) = tree.focus.filter(|id| tree.get(*id).is_some()) {
            let p: IRawElementProviderSimple = Node { id }.into();
            let _ = UiaRaiseAutomationEvent(&p, UIA_AutomationFocusChangedEventId);
        }
    }
}

impl IRawElementProviderSimple_Impl for Node_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading)
    }

    fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        if self.id == ROOT {
            return Err(Error::empty());
        }
        let ok = with_node(self.id, |n, _| match pattern {
            UIA_InvokePatternId => n.invokable() || n.role == Role::ComboBox,
            UIA_ValuePatternId => n.value.is_some(),
            UIA_TogglePatternId => n.toggled.is_some(),
            _ => false,
        })?;
        if ok {
            Ok(self.to_interface::<IUnknown>())
        } else {
            Err(Error::empty())
        }
    }

    fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        if self.id == ROOT {
            return Ok(match property {
                UIA_FrameworkIdPropertyId => VARIANT::from(BSTR::from("Win32")),
                _ => VARIANT::default(),
            });
        }
        with_node(self.id, |n, s| match property {
            UIA_NamePropertyId => VARIANT::from(BSTR::from(n.name.as_str())),
            UIA_ControlTypePropertyId => VARIANT::from(control_type(n.role).0),
            UIA_AutomationIdPropertyId => VARIANT::from(BSTR::from(format!("{:x}", n.id))),
            UIA_FrameworkIdPropertyId => VARIANT::from(BSTR::from("Win32")),
            UIA_IsEnabledPropertyId => VARIANT::from(n.enabled),
            UIA_IsKeyboardFocusablePropertyId => VARIANT::from(n.focusable),
            UIA_HasKeyboardFocusPropertyId => VARIANT::from(s.tree.focus == Some(n.id)),
            UIA_IsPasswordPropertyId => VARIANT::from(n.password),
            UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => VARIANT::from(true),
            UIA_AcceleratorKeyPropertyId => {
                VARIANT::from(BSTR::from(n.shortcut.as_deref().unwrap_or("")))
            }
            UIA_IsOffscreenPropertyId => VARIANT::from(!n.rect.is_positive()),
            _ => VARIANT::default(),
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        if self.id == ROOT {
            let hwnd = super::with_state(|s| s.hwnd()).ok_or_else(na)?;
            unsafe { UiaHostProviderFromHwnd(hwnd) }
        } else {
            Err(Error::empty())
        }
    }
}

impl IRawElementProviderFragment_Impl for Node_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let id = self.id;
        let target = super::with_state(|s| {
            let tree = &s.tree;
            match direction {
                NavigateDirection_Parent => parent_of(tree, id),
                NavigateDirection_FirstChild => children(tree, id).first().copied(),
                NavigateDirection_LastChild => children(tree, id).last().copied(),
                NavigateDirection_NextSibling | NavigateDirection_PreviousSibling => {
                    let p = parent_of(tree, id)?;
                    let sibs = children(tree, p);
                    let i = sibs.iter().position(|x| *x == id)?;
                    if direction == NavigateDirection_NextSibling {
                        sibs.get(i + 1).copied()
                    } else {
                        i.checked_sub(1).map(|j| sibs[j])
                    }
                }
                _ => None,
            }
        })
        .ok_or_else(na)?;
        target.map(fragment).ok_or_else(Error::empty)
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        if self.id == ROOT {
            return Ok(std::ptr::null_mut());
        }
        Ok(runtime_id(self.id))
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        if self.id == ROOT {
            return Ok(UiaRect::default());
        }
        with_node(self.id, |n, s| {
            let (ox, oy) = s.screen_origin();
            let k = s.scale() as f64;
            UiaRect {
                left: ox + n.rect.min.x as f64 * k,
                top: oy + n.rect.min.y as f64 * k,
                width: n.rect.width().max(0.0) as f64 * k,
                height: n.rect.height().max(0.0) as f64 * k,
            }
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        if self.id == ROOT {
            return Ok(());
        }
        act(A11yAction::Focus(self.id))
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(Node { id: ROOT }.into())
    }
}

impl IRawElementProviderFragmentRoot_Impl for Node_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let hit = super::with_state(|s| {
            let (ox, oy) = s.screen_origin();
            let k = s.scale() as f64;
            let p = crate::geom::pos2(((x - ox) / k) as f32, ((y - oy) / k) as f32);
            s.tree.hit(p).map_or(ROOT, |n| n.id)
        })
        .ok_or_else(na)?;
        Ok(fragment(hit))
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        match super::with_state(|s| s.tree.focus.filter(|f| s.tree.get(*f).is_some())).flatten() {
            Some(id) => Ok(fragment(id)),
            None => Err(Error::empty()),
        }
    }
}

impl IInvokeProvider_Impl for Node_Impl {
    fn Invoke(&self) -> Result<()> {
        let enabled = with_node(self.id, |n, _| n.enabled)?;
        if !enabled {
            return Err(HRESULT(UIA_E_ELEMENTNOTENABLED as i32).into());
        }
        act(A11yAction::Invoke(self.id))
    }
}

impl IValueProvider_Impl for Node_Impl {
    fn SetValue(&self, value: &PCWSTR) -> Result<()> {
        let read_only = with_node(self.id, |n, _| n.read_only)?;
        if read_only {
            return Err(HRESULT(UIA_E_ELEMENTNOTENABLED as i32).into());
        }
        let text = unsafe { value.to_string() }.unwrap_or_default();
        act(A11yAction::SetValue(self.id, text))
    }

    fn Value(&self) -> Result<BSTR> {
        with_node(self.id, |n, _| BSTR::from(n.value.as_deref().unwrap_or("")))
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        with_node(self.id, |n, _| n.read_only.into())
    }
}

impl IToggleProvider_Impl for Node_Impl {
    fn Toggle(&self) -> Result<()> {
        act(A11yAction::Toggle(self.id))
    }

    fn ToggleState(&self) -> Result<ToggleState> {
        with_node(self.id, |n, _| {
            if n.toggled == Some(true) {
                ToggleState_On
            } else {
                ToggleState_Off
            }
        })
    }
}
