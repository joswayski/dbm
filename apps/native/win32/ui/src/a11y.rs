//! The accessibility tree a frame records: every interactive control and
//! the containers around them, with names, values and states. The Win32
//! backend serves it through UI Automation.

use crate::geom::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Window,
    Group,
    Pane,
    Dialog,
    Button,
    /// A button that opens a menu or list.
    MenuButton,
    CheckBox,
    ComboBox,
    Edit,
    Text,
    Link,
    List,
    ListItem,
    Menu,
    MenuItem,
    Tab,
    TabItem,
    Tree,
    TreeItem,
    Table,
    Row,
    Cell,
    Header,
    ToolTip,
    ProgressBar,
    Separator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: u64,
    pub parent: Option<u64>,
    pub role: Role,
    pub name: String,
    /// A text field's text, a combo box's selection, a cell's value.
    pub value: Option<String>,
    pub rect: Rect,
    pub enabled: bool,
    pub focusable: bool,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
    pub expanded: Option<bool>,
    pub read_only: bool,
    pub password: bool,
    /// The keyboard shortcut a screen reader announces.
    pub shortcut: Option<String>,
}

impl Node {
    pub fn new(id: u64, role: Role, name: impl Into<String>, rect: Rect) -> Self {
        Node {
            id,
            parent: None,
            role,
            name: name.into(),
            value: None,
            rect,
            enabled: true,
            focusable: false,
            selected: None,
            toggled: None,
            expanded: None,
            read_only: false,
            password: false,
            shortcut: None,
        }
    }

    /// Controls that act when invoked (UIA's Invoke pattern).
    pub fn invokable(&self) -> bool {
        matches!(
            self.role,
            Role::Button
                | Role::MenuButton
                | Role::Link
                | Role::MenuItem
                | Role::ListItem
                | Role::TabItem
                | Role::TreeItem
                | Role::Row
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub focus: Option<u64>,
}

impl Tree {
    pub fn get(&self, id: u64) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn children(&self, id: Option<u64>) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.parent == id)
    }

    /// The deepest node containing `p`, preferring later (topmost) nodes.
    pub fn hit(&self, p: crate::geom::Pos2) -> Option<&Node> {
        let mut parent = None;
        let mut best = None;
        loop {
            let next = self
                .children(parent)
                .filter(|n| n.rect.contains(p))
                .last();
            match next {
                Some(n) => {
                    best = Some(n);
                    parent = Some(n.id);
                }
                None => return best,
            }
        }
    }
}
