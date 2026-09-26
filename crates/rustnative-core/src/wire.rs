//! The tree on the wire (`PLAN.md` Milestones 49 and 55): a [`Node`] as
//! serializable data, for a server-only component's output and a
//! server-interactive screen's updates. The client turns a [`WireNode`]
//! back into a `Node` and hands it to the ordinary reconciler, so a tree
//! rendered elsewhere is diffed and realized exactly like a local one.
//!
//! What travels: every node kind's content (text, values, tabs, controls,
//! a foreign kind), keys, layout, container style, grids and virtual
//! lists, the accessibility role, name, description, and focusability,
//! and the disabled, hidden, opacity, item index, command, cursor, and
//! shared-identity modifiers. What does not: a canvas's draw list (a
//! drawing is the client's to make), and compiled style — the client's
//! own theme styles what the server sends, as it styles everything else.
//!
//! ```
//! use rustnative_core::wire::WireNode;
//! use rustnative_core::Node;
//!
//! let tree = Node::column("list", [Node::label("title", "Notes"), Node::button("add", "Add")]);
//! let json = serde_json::to_string(&WireNode::from_node(&tree)).unwrap();
//! let back = serde_json::from_str::<WireNode>(&json).unwrap().into_node();
//! assert_eq!(back, tree);
//!
//! // The same tree in markup:
//! let markup = rustnative_core::rsx! {
//!     <Column key="list">
//!         <Label key="title" text="Notes" />
//!         <Button key="add" text="Add" />
//!     </Column>
//! };
//! assert_eq!(back, markup);
//! ```

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};

use crate::accessibility::{AccessibilityInfo, AccessibilityRole};
use crate::command::CommandId;
use crate::control::Control;
use crate::input::Cursor;
use crate::layout::{ColumnStyle, GridStyle, LayoutStyle, RowStyle};
use crate::node::Node;
use crate::virtualization::VirtualListStyle;

/// What kind of node, with its content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireKind {
    /// A label.
    Label {
        /// Its text.
        text: String,
    },
    /// A button.
    Button {
        /// Its text.
        text: String,
    },
    /// A text input.
    TextInput {
        /// Its value.
        value: String,
    },
    /// A tab bar.
    TabBar {
        /// The labels.
        labels: Vec<String>,
        /// The selected tab.
        selected: usize,
    },
    /// A native control.
    Control {
        /// Which, and its state.
        control: Control,
    },
    /// A canvas (its drawing does not travel).
    Canvas,
    /// A native surface for the application's own renderer.
    Surface,
    /// A foreign object of `foreign` kind.
    Foreign {
        /// Its kind.
        foreign: String,
    },
    /// A column.
    Column {
        /// Its style.
        style: ColumnStyle,
        /// Its grid tracks, if it is a grid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        grid: Option<GridStyle>,
        /// Its children.
        children: Vec<WireNode>,
    },
    /// A row.
    Row {
        /// Its style.
        style: RowStyle,
        /// Its children.
        children: Vec<WireNode>,
    },
}

/// A node as data; see the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireNode {
    /// Its key.
    pub key: String,
    /// Its kind and content.
    #[serde(flatten)]
    pub content: WireKind,
    /// Its layout.
    pub layout: LayoutStyle,
    /// Its accessible role.
    pub role: AccessibilityRole,
    /// Its accessible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its accessible description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether it takes focus.
    #[serde(default)]
    pub focusable: bool,
    /// Disabled.
    #[serde(default)]
    pub disabled: bool,
    /// Hidden.
    #[serde(default)]
    pub hidden: bool,
    /// Its opacity.
    #[serde(default = "one")]
    pub opacity: f32,
    /// Its index in a virtual list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_index: Option<usize>,
    /// Its virtual-list settings, if it is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtualization: Option<VirtualListStyle>,
    /// The command it invokes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Its pointer cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Its shared identity's key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_id: Option<String>,
}

const fn one() -> f32 {
    1.0
}

/// A node's key on the wire: its local key, prefixed `owner~` when a
/// component (not the root) owns it, so an event naming it finds the same
/// node when it comes back (see [`node_id`]).
#[must_use]
pub fn key_of(id: crate::identity::NodeId) -> String {
    let local = id.local_key().unwrap_or_else(|| format!("#{}", id.get()));
    match id.owner() {
        Some(owner) => format!("{}~{local}", owner.get()),
        None => local,
    }
}

/// The node a wire key names ([`key_of`]'s inverse).
#[must_use]
pub fn node_id(key: &str) -> crate::identity::NodeId {
    use crate::identity::{ComponentId, NodeId};
    if let Some((owner, local)) = key.split_once('~') {
        if let Ok(owner) = owner.parse::<u64>() {
            return NodeId::scoped(ComponentId::from_raw(owner), NodeId::from_key(local));
        }
    }
    NodeId::from_key(key)
}

/// A command id for a name read off the wire. Command ids are
/// `&'static str`; each distinct name is kept once for the life of the
/// process.
fn command(name: &str) -> CommandId {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names =
        NAMES.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner);
    let name = if let Some(existing) = names.get(name) {
        *existing
    } else {
        // ponytail: a leaked name per distinct command, bounded by the
        // commands an application declares; a server sending unbounded new
        // names would grow it, so refuse more than a few thousand.
        if names.len() > 4096 {
            return CommandId::new("rustnative.wire.too-many-commands");
        }
        let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
        names.insert(leaked);
        leaked
    };
    CommandId::new(name)
}

impl WireNode {
    /// `node` as data.
    #[must_use]
    pub fn from_node(node: &Node) -> Self {
        let content = match node {
            Node::Label(label) => WireKind::Label { text: label.text().to_owned() },
            Node::Button(button) => WireKind::Button { text: button.text().to_owned() },
            Node::TextInput(input) => WireKind::TextInput { value: input.value().to_owned() },
            Node::TabBar(bar) => WireKind::TabBar {
                labels: bar.tabs().labels().to_vec(),
                selected: bar.tabs().selected(),
            },
            Node::Control(_) => WireKind::Control {
                control: node.control_state().cloned().unwrap_or(Control::Separator),
            },
            Node::Canvas(_) => WireKind::Canvas,
            Node::Surface(_) => node
                .foreign_kind()
                .map_or(WireKind::Surface, |kind| WireKind::Foreign { foreign: kind.to_owned() }),
            Node::Column(column) => WireKind::Column {
                style: node.column_style().unwrap_or_default(),
                grid: column.grid().cloned(),
                children: column.children().iter().map(Self::from_node).collect(),
            },
            Node::Row(row) => WireKind::Row {
                style: node.row_style().unwrap_or_default(),
                children: row.children().iter().map(Self::from_node).collect(),
            },
        };
        let accessibility = node.accessibility();
        Self {
            key: key_of(node.id()),
            content,
            layout: node.layout(),
            role: accessibility.role(),
            name: accessibility.name_hint().map(str::to_owned),
            description: accessibility.description_hint().map(str::to_owned),
            focusable: accessibility.is_focusable(),
            disabled: node.is_disabled(),
            hidden: node.is_hidden(),
            opacity: node.opacity(),
            item_index: node.item_index(),
            virtualization: node.virtualization(),
            command: node.command().map(|command| command.name().to_owned()),
            cursor: node.cursor(),
            shared_id: node.shared_id().map(key_of),
        }
    }

    /// The node this describes.
    #[must_use]
    pub fn into_node(self) -> Node {
        let key = self.key.as_str();
        let layout = self.layout;
        let mut node = match self.content {
            WireKind::Label { text } => Node::label_with_layout(key, text, layout),
            WireKind::Button { text } => Node::button_with_layout(key, text, layout),
            WireKind::TextInput { value } => Node::text_input_with_layout(key, value, layout),
            WireKind::TabBar { labels, selected } => Node::tab_bar(key, labels, selected, layout),
            WireKind::Control { control } => Node::control_with_layout(key, control, layout),
            WireKind::Canvas => Node::canvas(key, crate::graphics::DrawList::default(), layout),
            WireKind::Surface => Node::native_surface(key, layout),
            WireKind::Foreign { foreign } => Node::foreign(key, foreign, layout),
            WireKind::Column { style, grid, children } => {
                let children = children.into_iter().map(Self::into_node);
                match (grid, self.virtualization) {
                    (Some(grid), _) => Node::grid(key, grid, layout, children),
                    (None, Some(list)) => {
                        Node::virtual_list_with_layout(key, list, layout, children)
                    }
                    (None, None) => Node::column_with_layout(key, children, layout, style),
                }
            }
            WireKind::Row { style, children } => {
                let children = children.into_iter().map(Self::into_node);
                match self.virtualization {
                    Some(list) => Node::virtual_list_with_layout(key, list, layout, children),
                    None => Node::row_with_layout(key, children, layout, style),
                }
            }
        };
        // The constructor already describes what the node is (a control's
        // state included); only a description that differs replaces it.
        let built = node.accessibility();
        let same = built.role() == self.role
            && built.name_hint() == self.name.as_deref()
            && built.description_hint() == self.description.as_deref()
            && built.is_focusable() == self.focusable;
        if !same {
            let mut info = AccessibilityInfo::new(self.role).focusable(self.focusable);
            if let Some(name) = self.name {
                info = info.name(name);
            }
            if let Some(description) = self.description {
                info = info.description(description);
            }
            node = node.with_accessibility(info);
        }
        node = node.disabled(self.disabled).hidden(self.hidden);
        if (self.opacity - 1.0).abs() > f32::EPSILON {
            node = node.with_opacity(self.opacity);
        }
        if let Some(index) = self.item_index {
            node = node.with_item_index(index);
        }
        if let Some(name) = self.command {
            node = node.with_command(command(&name));
        }
        if let Some(cursor) = self.cursor {
            node = node.with_cursor(cursor);
        }
        if let Some(shared) = self.shared_id {
            node = node.with_shared_id(shared);
        }
        node
    }
}
