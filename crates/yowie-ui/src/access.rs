//! The accessibility tree: every widget of a frame as data.

use yowie_core::Rect;

use crate::id::WidgetId;

/// What a widget is, in the terms assistive technology uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// A movable window.
    Window,
    /// A fixed panel.
    Panel,
    /// A modal dialog.
    Dialog,
    /// A grouping container such as a scroll area.
    Group,
    /// Static text.
    Label,
    /// A push button.
    Button,
    /// A check box.
    CheckBox,
    /// One option of a radio group.
    RadioButton,
    /// A float or integer slider.
    Slider,
    /// A progress bar.
    ProgressBar,
    /// A single-line text editor.
    TextField,
    /// A multi-line text editor.
    TextArea,
    /// A drop-down selector.
    ComboBox,
    /// A list of selectable items.
    ListBox,
    /// One item of a list, combo box or menu.
    ListItem,
    /// A table.
    Table,
    /// A table's header or body row.
    Row,
    /// A table cell.
    Cell,
    /// A tree.
    Tree,
    /// A node of a tree.
    TreeItem,
    /// A row of tabs.
    TabList,
    /// One tab.
    Tab,
    /// A menu bar.
    MenuBar,
    /// A menu.
    Menu,
    /// A menu entry.
    MenuItem,
    /// A tooltip.
    Tooltip,
    /// A picture.
    Image,
    /// A colour swatch.
    ColorSwatch,
    /// A section heading that expands.
    Heading,
    /// A divider.
    Separator,
}

/// A widget's state flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessState {
    /// The widget has keyboard focus.
    pub focused: bool,
    /// The widget ignores input.
    pub disabled: bool,
    /// Check boxes, radio buttons and checked menu items: whether on.
    pub checked: Option<bool>,
    /// Trees, headings, menus and combo boxes: whether open.
    pub expanded: Option<bool>,
    /// Items: whether selected.
    pub selected: bool,
}

/// One widget of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessNode {
    /// The widget's id.
    pub id: WidgetId,
    /// What it is.
    pub role: Role,
    /// Its label or caption.
    pub name: String,
    /// Its value as text: a slider's number, a field's contents.
    pub value: Option<String>,
    /// Its flags.
    pub state: AccessState,
    /// Where it was drawn, in view units.
    pub rect: Rect,
    /// The index of the containing node, if any.
    pub parent: Option<usize>,
}

/// Helpers for reading a frame's nodes.
pub trait AccessTree {
    /// The first node with `role` and `name`.
    fn find(&self, role: Role, name: &str) -> Option<&AccessNode>;
    /// The indices of `index`'s children.
    fn children(&self, index: usize) -> Vec<usize>;
}

impl AccessTree for [AccessNode] {
    fn find(&self, role: Role, name: &str) -> Option<&AccessNode> {
        self.iter().find(|n| n.role == role && n.name == name)
    }

    fn children(&self, index: usize) -> Vec<usize> {
        self.iter()
            .enumerate()
            .filter(|(_, n)| n.parent == Some(index))
            .map(|(i, _)| i)
            .collect()
    }
}
