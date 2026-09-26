//! What the inspector reads from, and edits through, a component tree
//! (`PLAN.md` Milestone 44).

use std::collections::BTreeMap;

use super::super::context::QueuedMessage;
use super::ComponentTree;
use crate::identity::{ComponentId, NodeId};
use crate::inspect::{ComponentInfo, PassInfo, RenderInfo};

impl ComponentTree {
    /// Every component, ordered by key path.
    #[must_use]
    pub fn inspect_components(&self) -> Vec<ComponentInfo> {
        let mut components: Vec<ComponentInfo> = self
            .components
            .iter()
            .map(|(id, entry)| ComponentInfo {
                id: id.get(),
                path: self.paths.get(id).cloned().unwrap_or_default(),
                type_name: entry.component.type_name().to_owned(),
                parent: self.parents.get(id).map(|parent| parent.get()),
                state: entry.component.inspect(),
                tasks: entry.task_scope.task_count(),
            })
            .collect();
        components.sort_by(|a, b| a.path.cmp(&b.path));
        components
    }

    /// Each inspectable component's state, by key path.
    #[must_use]
    pub fn inspected_states(&self) -> BTreeMap<String, serde_json::Value> {
        self.components
            .iter()
            .filter_map(|(id, entry)| {
                Some((self.paths.get(id)?.clone(), entry.component.inspect()?))
            })
            .collect()
    }

    /// The root component's key path: its type's name.
    #[must_use]
    pub fn root_path(&self) -> &str {
        self.paths.get(&ComponentId::ROOT).map_or("", String::as_str)
    }

    /// Each inspectable component's state, by key path relative to the
    /// root (`""` for the root, `/child` below it) — how recordings name
    /// components, so a recording made in one binary replays in another
    /// whose root type has a different path.
    #[must_use]
    pub fn relative_states(&self) -> BTreeMap<String, serde_json::Value> {
        let root = self.root_path();
        self.inspected_states()
            .into_iter()
            .map(|(path, state)| (path.strip_prefix(root).unwrap_or(&path).to_owned(), state))
            .collect()
    }

    /// Sets `field` of the component at `path` to `value` through the
    /// message the component names for it ([`crate::Component::edit`]),
    /// delivered and rendered like any other message.
    ///
    /// # Errors
    ///
    /// No component has that path, or it does not make that field
    /// editable.
    pub fn edit_component(
        &mut self,
        path: &str,
        field: &str,
        value: &serde_json::Value,
    ) -> Result<(), String> {
        let id = self.component_at(path).ok_or_else(|| format!("no component at `{path}`"))?;
        let message = self
            .components
            .get(&id)
            .and_then(|entry| entry.component.edit(field, value))
            .ok_or_else(|| format!("`{path}` does not make `{field}` editable to {value}"))?;
        self.message_sink.borrow_mut().push_back(QueuedMessage {
            target: id,
            message,
            priority: crate::scheduler::Priority::Normal,
        });
        self.drain_messages();
        let _ = self.render_pass(false);
        Ok(())
    }

    /// The component at key path `path`.
    #[must_use]
    pub fn component_at(&self, path: &str) -> Option<ComponentId> {
        self.paths.iter().find(|(_, candidate)| *candidate == path).map(|(id, _)| *id)
    }

    /// The key path of the component that rendered node `id`.
    #[must_use]
    pub fn node_component_path(&self, id: NodeId) -> Option<String> {
        self.paths.get(&id.owner().unwrap_or(ComponentId::ROOT)).cloned()
    }

    /// The node with key `key` — rendered by the component at `component`,
    /// when given — first in document order.
    #[must_use]
    pub fn find_node(&self, component: Option<&str>, key: &str) -> Option<NodeId> {
        let owner = match component {
            Some(path) => Some(self.component_at(path)?),
            None => None,
        };
        let mut found = None;
        self.root_view.as_ref()?.visit(&mut |node, _, _| {
            let id = node.id();
            if found.is_none()
                && id.local_key().as_deref() == Some(key)
                && owner.is_none_or(|owner| id.owner().unwrap_or(ComponentId::ROOT) == owner)
            {
                found = Some(id);
            }
        });
        found
    }

    /// What the most recent render pass did: who rendered and why, and who
    /// was skipped.
    #[must_use]
    pub fn last_pass(&self) -> PassInfo {
        let rendered: Vec<RenderInfo> = self
            .render_log
            .iter()
            .map(|record| RenderInfo {
                component: record.path.clone(),
                cause: record.cause.to_string(),
            })
            .collect();
        let mut skipped: Vec<String> = self
            .components
            .keys()
            .filter(|id| !self.render_log.iter().any(|record| record.component == **id))
            .filter_map(|id| self.paths.get(id).cloned())
            .collect();
        skipped.sort();
        PassInfo { rendered, skipped }
    }

    /// Replaces the message catalogues and re-renders exactly the
    /// components that show messages (those that read the locale).
    pub fn set_catalogues(&mut self, catalogues: std::sync::Arc<crate::i18n::Catalogues>) {
        self.services = self.services.clone().with_catalogues(catalogues);
        let locale = crate::environment::keys::LOCALE.name();
        let readers: Vec<ComponentId> = self
            .env_seen
            .iter()
            .filter(|(_, seen)| seen.contains_key(locale))
            .map(|(id, _)| *id)
            .collect();
        for id in readers {
            self.dirty.entry(id).or_insert(crate::component::RenderCause::Environment(locale));
        }
        let _ = self.render_pass(false);
    }

    /// What node `id`'s style conditions are evaluated against: its
    /// rendering component's environment.
    pub(crate) fn condition_env(&self, id: NodeId) -> rustnative_style::ConditionEnv {
        let owner = self.node_owners.get(&id).map(|(component, _)| *component);
        self.style_env(owner).condition
    }

    /// The last render's output as authored, before style resolution
    /// folded any declarations into it.
    pub(crate) fn authored_view(&self) -> Option<crate::node::Node> {
        self.unresolved_root.clone().or_else(|| self.root_view.clone())
    }
}
