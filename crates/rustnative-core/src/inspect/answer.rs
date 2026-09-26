//! How an [`Application`] answers the protocol, and the hooks that feed
//! its trace, history, and recording.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use rustnative_style::{SetKind, StyleProperty, StyleSupport};

use super::record::Recorder;
use super::server::InspectServer;
use super::trace::{PassInfo, TraceKind};
use super::{
    CapabilityReport, Endpoint, Hello, HttpTape, InspectBackend, LayoutExplanation, NodeInfo,
    Origin, OverlayMode, Recording, Refusal, Reply, Request, StyleExplanation, StyleSource,
    TaskInfo, TraceEntry, node_name,
};
use crate::application::Application;
use crate::component::ComponentTree;
use crate::event::Event;
use crate::graphics::DrawList;
use crate::identity::{NodeId, WindowId};
use crate::layout::{LayoutEngine, Rect, SizeMode};
use crate::node::Node;
use crate::reconcile::{TreeNode, TreeSnapshot};
use crate::style::{ControlState, StyleOverride, VisualStyle};

/// The environment variable that turns inspection on for a backend's run.
pub const INSPECT_VARIABLE: &str = "RUSTNATIVE_INSPECT";

impl Application {
    /// Starts an inspection server on `bind` (loopback when `None`, on a
    /// port the system picks), woken through the primary window's
    /// scheduler; while one is running, answers its endpoint. A backend answers what arrives by calling
    /// [`Self::poll_inspection`] when woken.
    ///
    /// # Errors
    ///
    /// The address cannot be bound.
    pub fn enable_inspection(&mut self, bind: Option<SocketAddr>) -> io::Result<Endpoint> {
        // Already listening where asked: the same endpoint.
        if let Some(server) = &self.inspection.server {
            if bind.is_none_or(|bind| bind == server.endpoint().addr) {
                return Ok(server.endpoint().clone());
            }
        }
        let bind = bind.unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)));
        let wake: Arc<dyn Fn() + Send + Sync> = self.scheduler().host_waker();
        let server = InspectServer::start(bind, wake)?;
        let endpoint = server.endpoint().clone();
        self.inspection.server = Some(server);
        self.inspection.enabled = true;
        Ok(endpoint)
    }

    /// Starts inspection when the environment asks for it:
    /// `RUSTNATIVE_INSPECT=1` for loopback, or `RUSTNATIVE_INSPECT=<addr>`
    /// to bind that address — then publishes the endpoint for
    /// `rustnative inspect` and says where on standard error. What every
    /// backend calls as it starts running.
    pub fn enable_inspection_from_env(&mut self) -> Option<Endpoint> {
        let value = std::env::var(INSPECT_VARIABLE).ok()?;
        let bind = match value.trim() {
            "" | "0" => return None,
            "1" => None,
            address => {
                let Ok(address) = address.parse() else {
                    eprintln!("rustnative: {INSPECT_VARIABLE}={address} is not `1` or an address");
                    return None;
                };
                Some(address)
            }
        };
        match self.enable_inspection(bind) {
            Ok(endpoint) => {
                let published = self.inspection.server.as_mut().map(InspectServer::publish);
                match published {
                    Some(Ok(path)) => eprintln!(
                        "rustnative: inspection listening on {} (endpoint in {})",
                        endpoint.addr,
                        path.display()
                    ),
                    _ => eprintln!("rustnative: inspection listening on {}", endpoint.addr),
                }
                Some(endpoint)
            }
            Err(error) => {
                eprintln!("rustnative: inspection could not start: {error}");
                None
            }
        }
    }

    /// Answers every request waiting on the inspection server. Returns
    /// whether any was answered — after which the backend realizes the
    /// tree, since an edit or an overlay may have changed it.
    pub fn poll_inspection(&mut self, backend: &dyn InspectBackend) -> bool {
        let Some(server) = self.inspection.server.take() else { return false };
        let answered = server.poll(|request| self.inspect(request, backend));
        self.inspection.server = Some(server);
        answered > 0
    }

    /// Replaces the theme's tokens with those `css` (an `app.css`) declares,
    /// over the default theme's, and re-resolves every window's style —
    /// the live half of theme editing. Returns how many tokens it set.
    ///
    /// # Errors
    ///
    /// The file does not parse.
    pub fn apply_style_file(&mut self, css: &str) -> Result<usize, String> {
        let vocabulary = rustnative_style::Vocabulary::with_style_file(css).map_err(|errors| {
            errors.iter().map(|error| error.message.clone()).collect::<Vec<_>>().join("; ")
        })?;
        let tokens = vocabulary.token_table();
        let count = vocabulary.token_names().count();
        let theme = self.theme().clone().with_tokens(tokens);
        self.set_theme(theme);
        Ok(count)
    }

    /// Replaces the message catalogues in every window; only the components
    /// that show messages re-render.
    pub fn set_catalogues(&mut self, catalogues: Arc<crate::i18n::Catalogues>) {
        for id in self.window_ids() {
            if let Some(tree) = self.components_mut(id) {
                tree.set_catalogues(Arc::clone(&catalogues));
            }
        }
        self.replace_services(|services| services.with_catalogues(catalogues));
    }

    /// Whether an inspector asked the application to close since the last
    /// call — a backend closes its windows when it did.
    pub fn take_quit_request(&mut self) -> bool {
        std::mem::take(&mut self.inspection.quit)
    }

    /// Shows the in-application overlay in `mode`, or hides it.
    pub fn set_overlay(&mut self, mode: Option<OverlayMode>) {
        self.inspection.overlay = mode;
    }

    /// The overlay being shown.
    #[must_use]
    pub const fn overlay(&self) -> Option<OverlayMode> {
        self.inspection.overlay
    }

    /// The overlay to draw over window `id`, whose nodes the backend laid
    /// out at `rects` (window coordinates), or `None` when it is hidden.
    #[must_use]
    pub fn overlay_draw_list(
        &self,
        id: WindowId,
        rects: &HashMap<NodeId, Rect>,
    ) -> Option<DrawList> {
        let mode = self.inspection.overlay?;
        let width = self.window_state(id).map_or(0, |state| state.size().width);
        let trace: Vec<&TraceEntry> =
            self.inspection.trace.iter().filter(|entry| entry.window == id.get()).collect();
        Some(super::overlay::draw(mode, rects, &trace, width))
    }

    /// The trace so far, oldest first. Empty until inspection is on.
    #[must_use]
    pub fn trace(&self) -> Vec<TraceEntry> {
        self.inspection.trace.iter().cloned().collect()
    }

    /// Starts recording input; text entered into a node whose key contains
    /// one of `redact` is recorded as `[redacted]`.
    pub fn start_recording(&mut self, redact: Vec<String>) {
        let now = self.services().clock().now();
        self.inspection.recorder = Some(Recorder::new(now, redact));
    }

    /// Stops recording, returning what was recorded, or `None` when
    /// nothing was.
    pub fn stop_recording(&mut self) -> Option<Recording> {
        let recorder = self.inspection.recorder.take()?;
        let http = self.inspection.http.as_ref().map(HttpTape::take).unwrap_or_default();
        let final_state = self.components().relative_states();
        Some(recorder.finish(http, final_state))
    }

    /// Has recordings include the exchanges on `tape` — the tape of the
    /// [`super::RecordingHttp`] installed as this application's HTTP
    /// service.
    pub fn record_http(&mut self, tape: HttpTape) {
        self.inspection.http = Some(tape);
    }

    pub(crate) fn record_input(&mut self, id: WindowId, event: &Event) {
        let now = self.services().clock().now();
        let Some(mut recorder) = self.inspection.recorder.take() else { return };
        if let Some(tree) = self.components_for(id) {
            recorder.record(now, tree, event);
        }
        self.inspection.recorder = Some(recorder);
    }

    /// Traces a change to window `id` that began at `started`: `kind` is
    /// given the render pass it caused.
    pub(crate) fn trace_change(
        &mut self,
        id: WindowId,
        started: Instant,
        kind: impl FnOnce(PassInfo) -> TraceKind,
    ) {
        let Some(tree) = self.components_for(id) else { return };
        let pass = tree.last_pass();
        let states = tree.inspected_states();
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let kind = kind(pass);
        let cause = match &kind {
            TraceKind::Event { event, .. } => event.clone(),
            TraceKind::Tasks { .. } => "tasks".to_owned(),
            TraceKind::Edit { component, field, .. } => format!("edit {component}.{field}"),
            TraceKind::Failure { component, .. } => format!("failure {component}"),
        };
        let seq = self.inspection.push(id.get(), micros, kind);
        self.inspection.remember(seq, cause, states);
    }

    /// Answers `request`, with `backend` supplying what only it knows.
    pub fn inspect(&mut self, request: &Request, backend: &dyn InspectBackend) -> Reply {
        // Anyone asking turns tracing on from here.
        self.inspection.enabled = true;
        let window = |window: &Option<u64>| {
            window.map_or(WindowId::PRIMARY, |id| {
                self.window_ids()
                    .into_iter()
                    .find(|open| open.get() == id)
                    .unwrap_or(WindowId::PRIMARY)
            })
        };
        match request {
            Request::Hello => Reply::of(&Hello {
                version: super::PROTOCOL_VERSION,
                backend: backend.name().to_owned(),
                windows: self.window_ids().iter().map(|id| id.get()).collect(),
            }),
            Request::Tree { window: id } => self.with_tree(window(id), |tree, _| {
                let view = tree.view();
                Reply::of(&node_info(
                    tree,
                    &view,
                    &TreeSnapshot::from_node(&view).unwrap_or_default(),
                ))
            }),
            Request::Components { window: id } => {
                self.with_tree(window(id), |tree, _| Reply::of(&tree.inspect_components()))
            }
            Request::Realized { window: id } => Reply::of(&backend.realized(window(id))),
            Request::State { path, window: id } => self.with_tree(window(id), |tree, _| {
                tree.inspect_components()
                    .into_iter()
                    .find(|component| component.path == *path)
                    .map_or_else(
                        || Reply::Error(format!("no component at `{path}`")),
                        |component| Reply::of(&component.state),
                    )
            }),
            Request::SetState { path, field, value, window: id } => {
                let id = window(id);
                let started = Instant::now();
                let edited =
                    self.components_mut(id).map(|tree| tree.edit_component(path, field, value));
                match edited {
                    None => Reply::Error("that window is not open".into()),
                    Some(Err(error)) => Reply::Error(error),
                    Some(Ok(())) => {
                        let (component, field) = (path.clone(), field.clone());
                        self.trace_change(id, started, |pass| TraceKind::Edit {
                            component,
                            field,
                            pass,
                        });
                        self.with_tree(id, |tree, _| {
                            Reply::of(
                                &tree.inspect_components().into_iter().find(|c| c.path == *path),
                            )
                        })
                    }
                }
            }
            Request::ExplainLayout { node, window: id } => {
                let id = window(id);
                let size = self.window_state(id).map(crate::WindowState::size);
                let rects = backend.rects(id);
                self.with_tree(id, |tree, theme| {
                    explain_layout(tree, theme, node, size.unwrap_or_default(), rects)
                })
            }
            Request::ExplainStyle { node, window: id } => {
                self.with_tree(window(id), |tree, theme| explain_style(tree, theme, node))
            }
            Request::Trace { since } => Reply::of(
                &self
                    .inspection
                    .trace
                    .iter()
                    .filter(|entry| entry.seq >= *since)
                    .collect::<Vec<_>>(),
            ),
            Request::Tasks { window: id } => self.with_tree(window(id), |tree, _| {
                Reply::of(
                    &tree
                        .inspect_components()
                        .into_iter()
                        .map(|component| TaskInfo {
                            component: component.path,
                            tasks: component.tasks,
                        })
                        .collect::<Vec<_>>(),
                )
            }),
            Request::Lifetimes => Reply::of(&backend.lifetimes()),
            Request::Stores => Reply::of(&crate::state::inspect_stores()),
            Request::Jobs => Reply::Error("this process has no job queue; ask its server".into()),
            Request::Capabilities => Reply::of(&self.capability_report(backend)),
            Request::Mappers => Reply::of(&backend.mappers()),
            Request::History { component } => Reply::of(
                &self
                    .inspection
                    .history
                    .iter()
                    .map(|entry| {
                        let mut entry = entry.clone();
                        if let Some(component) = component {
                            entry.states.retain(|path, _| path == component);
                        }
                        entry
                    })
                    .collect::<Vec<_>>(),
            ),
            Request::Overlay { mode } => {
                self.set_overlay(*mode);
                Reply::of(mode)
            }
            Request::StartRecording { redact } => {
                self.start_recording(redact.clone());
                Reply::of(&true)
            }
            Request::StopRecording => match self.stop_recording() {
                Some(recording) => Reply::of(&recording),
                None => Reply::Error("nothing is being recorded".into()),
            },
            Request::SetStyleFile { css } => match self.apply_style_file(css) {
                Ok(tokens) => Reply::of(&tokens),
                Err(error) => Reply::Error(error),
            },
            Request::SetCatalogue { locale, ftl } => {
                let Some(current) = self.services().catalogues().cloned() else {
                    return Reply::Error("the application has no catalogues".into());
                };
                match current.with_file(locale, ftl) {
                    Ok(replaced) => {
                        self.set_catalogues(Arc::new(replaced));
                        Reply::of(&true)
                    }
                    Err(errors) => Reply::Error(errors.join("; ")),
                }
            }
            Request::Quit => {
                self.inspection.quit = true;
                Reply::of(&true)
            }
        }
    }

    fn with_tree(
        &self,
        id: WindowId,
        answer: impl FnOnce(&ComponentTree, &crate::style::Theme) -> Reply,
    ) -> Reply {
        self.components_for(id).map_or_else(
            || Reply::Error(format!("window {} is not open", id.get())),
            |tree| answer(tree, self.theme()),
        )
    }

    fn capability_report(&self, backend: &dyn InspectBackend) -> CapabilityReport {
        use crate::capability::Capability as C;
        let advertised = backend.capabilities();
        let every = C::ALL;
        let mut refused: Vec<Refusal> = every
            .iter()
            .filter(|capability| !advertised.supports(**capability))
            .map(|capability| Refusal {
                what: format!("{capability:?}"),
                why: format!("the {} backend does not advertise it", backend.name()),
            })
            .collect();
        let services = self.services();
        let provided = [
            ("http", services.http().is_some()),
            ("storage", services.storage().is_some()),
            ("state_store", services.state_store().is_some()),
        ];
        refused.extend(provided.iter().filter(|(_, present)| !present).map(|(service, _)| {
            Refusal {
                what: format!("service `{service}`"),
                why: "the application did not provide it (`Services::with_…`)".into(),
            }
        }));
        let style = backend.style_capabilities();
        CapabilityReport {
            backend: backend.name().to_owned(),
            advertised: advertised.iter().map(|capability| format!("{capability:?}")).collect(),
            refused,
            style: style
                .rows()
                .map(|(property, support)| Refusal {
                    what: property.to_string(),
                    why: match support {
                        StyleSupport::Realized => "realized".to_owned(),
                        StyleSupport::Approximated(how) => format!("approximated: {how}"),
                        StyleSupport::Unavailable(why) => format!("unavailable: {why}"),
                    },
                })
                .collect(),
            units: backend.unit_mapping().map(|units| {
                BTreeMap::from([
                    ("host_unit".to_owned(), units.host_unit.to_owned()),
                    ("pixel".to_owned(), units.pixel.to_owned()),
                    ("rem".to_owned(), units.rem.to_owned()),
                    ("rounding".to_owned(), units.rounding.to_owned()),
                ])
            }),
            services: provided
                .iter()
                .filter(|(_, present)| *present)
                .map(|(service, _)| (*service).to_owned())
                .collect(),
        }
    }
}

fn node_info(tree: &ComponentTree, node: &Node, snapshot: &TreeSnapshot) -> NodeInfo {
    let id = node.id();
    let realized = snapshot.get(id);
    NodeInfo {
        id: node_name(id),
        key: id.local_key(),
        kind: format!("{:?}", node.kind()),
        text: realized.and_then(|realized| realized.text.clone()),
        component: tree.node_component_path(id).unwrap_or_default(),
        classes: node.declarations().iter().map(|set| set_text(*set)).collect(),
        hidden: node.is_hidden(),
        children: node.child_nodes().iter().map(|child| node_info(tree, child, snapshot)).collect(),
    }
}

/// A declaration set as its author wrote it, when recorded.
fn set_text(set: rustnative_style::DeclarationSet) -> String {
    let mut sources: Vec<&str> =
        (0..set.declarations().len()).filter_map(|index| set.source(index)).collect();
    sources.dedup();
    if sources.is_empty() {
        return set.to_string();
    }
    sources.join(if set.kind() == SetKind::Declarations { "; " } else { " " })
}

/// The node `name` names: an id, `component path::key`, or a key.
fn find(tree: &ComponentTree, name: &str) -> Option<NodeId> {
    let view = tree.view();
    if let Ok(id) = name.parse::<u128>() {
        let mut found = None;
        view.visit(&mut |node, _, _| {
            if node.id().get() == id {
                found = Some(node.id());
            }
        });
        if found.is_some() {
            return found;
        }
    }
    match name.rsplit_once("::") {
        Some((component, key)) => tree.find_node(Some(component), key),
        None => tree.find_node(None, name),
    }
}

fn find_node(node: &Node, id: NodeId) -> Option<&Node> {
    if node.id() == id {
        return Some(node);
    }
    node.child_nodes().iter().find_map(|child| find_node(child, id))
}

fn size_reason(axis: &str, mode: SizeMode, value: i32, available: Option<i32>) -> String {
    match mode {
        SizeMode::Fill => match available {
            Some(available) => format!(
                "{axis} Fill: takes the {axis} its parent gives it ({value} of {available} available)"
            ),
            None => format!("{axis} Fill: takes the whole window's {axis} ({value})"),
        },
        SizeMode::Fixed(fixed) if fixed == value => {
            format!("{axis} Fixed({fixed}): exactly {fixed}")
        }
        SizeMode::Fixed(fixed) => {
            format!("{axis} Fixed({fixed}), then held to {value} by its constraints")
        }
        SizeMode::Auto => format!("{axis} Auto: sized to its content, which measured {value}"),
    }
}

fn explain_layout(
    tree: &ComponentTree,
    theme: &crate::style::Theme,
    name: &str,
    size: crate::layout::Size,
    rects: Option<HashMap<NodeId, Rect>>,
) -> Reply {
    let Some(id) = find(tree, name) else { return Reply::Error(format!("no node `{name}`")) };
    let view = tree.view();
    let Ok(snapshot) = TreeSnapshot::from_node_with_theme(&view, theme) else {
        return Reply::Error("the tree does not form a valid snapshot".into());
    };
    let realized = rects.is_some();
    let rects = rects.unwrap_or_else(|| LayoutEngine::new().layout(&snapshot, size));
    let (Some(node), Some(rect)) = (snapshot.get(id), rects.get(&id).copied()) else {
        return Reply::Error(format!(
            "`{name}` is not laid out (hidden, or outside a virtual list's window)"
        ));
    };
    let mut reasons = Vec::new();
    let parent = node.parent.and_then(|parent| snapshot.get(parent));
    let parent_rect = node.parent.and_then(|parent| rects.get(&parent).copied());
    match parent {
        Some(parent) => {
            reasons.push(format!(
                "its parent `{}` is a {:?}{}: it is child {} and was placed at ({}, {})",
                parent.id.local_key().unwrap_or_default(),
                parent.kind,
                container_text(parent),
                node.index,
                rect.x,
                rect.y
            ));
        }
        None => reasons
            .push(format!("it is the window's root, laid out in {}×{}", size.width, size.height)),
    }
    reasons.push(size_reason("width", node.layout.width, rect.width, parent_rect.map(|r| r.width)));
    reasons.push(size_reason(
        "height",
        node.layout.height,
        rect.height,
        parent_rect.map(|r| r.height),
    ));
    let constraints = node.layout.constraints;
    if constraints != crate::layout::Constraints::default() {
        reasons.push(format!(
            "constraints: width {}–{}, height {}–{}",
            constraints.min_width(),
            constraints.max_width().map_or("∞".into(), |max| max.to_string()),
            constraints.min_height(),
            constraints.max_height().map_or("∞".into(), |max| max.to_string()),
        ));
    }
    if node.layout.margin != crate::layout::EdgeInsets::default() {
        reasons.push(format!("margin {:?}", node.layout.margin));
    }
    if let Some(alignment) = node.layout.align_self {
        reasons.push(format!("aligned {alignment:?} within its parent's cross axis"));
    }
    if let Some(direction) = node.layout.direction {
        reasons.push(format!("lays its children out {direction:?}"));
    }
    if let Some(text) = &node.text {
        reasons.push(format!(
            "its content is the text {text:?}, in {:?}",
            node.visual_style.properties().typography_override()
        ));
    }
    Reply::of(&LayoutExplanation {
        node: node_name(id),
        key: id.local_key(),
        rect: [rect.x, rect.y, rect.width, rect.height],
        realized,
        reasons,
    })
}

fn container_text(node: &TreeNode) -> String {
    match (&node.column_style, &node.row_style) {
        (Some(column), _) => format!(" ({column:?})"),
        (_, Some(row)) => format!(" ({row:?})"),
        _ => String::new(),
    }
}

/// A visual property's value in `style`, as text.
fn visual(style: &VisualStyle, property: StyleProperty) -> Option<String> {
    let typography = style.typography_override();
    let padding = style.padding_override();
    match property {
        StyleProperty::Foreground => style.foreground_override().map(|c| format!("{c:?}")),
        StyleProperty::Background => style.background_override().map(|c| format!("{c:?}")),
        StyleProperty::BorderColor => style.border_override().map(|c| format!("{c:?}")),
        StyleProperty::BorderRadius => style.border_radius_override().map(|r| r.to_string()),
        StyleProperty::FontSize => typography.map(|t| t.size.to_string()),
        StyleProperty::FontWeight => typography.map(|t| t.weight.to_string()),
        StyleProperty::FontFamily => typography.map(|t| t.family.clone()),
        StyleProperty::Shadow => style.shadow_override().map(|layers| format!("{layers:?}")),
        StyleProperty::PaddingTop => padding.map(|p| p.top.to_string()),
        StyleProperty::PaddingEnd => padding.map(|p| p.end.to_string()),
        StyleProperty::PaddingBottom => padding.map(|p| p.bottom.to_string()),
        StyleProperty::PaddingStart => padding.map(|p| p.start.to_string()),
        _ => None,
    }
}

const VISUAL: [StyleProperty; 12] = [
    StyleProperty::Foreground,
    StyleProperty::Background,
    StyleProperty::BorderColor,
    StyleProperty::BorderRadius,
    StyleProperty::FontSize,
    StyleProperty::FontWeight,
    StyleProperty::FontFamily,
    StyleProperty::Shadow,
    StyleProperty::PaddingTop,
    StyleProperty::PaddingEnd,
    StyleProperty::PaddingBottom,
    StyleProperty::PaddingStart,
];

fn explain_style(tree: &ComponentTree, theme: &crate::style::Theme, name: &str) -> Reply {
    let Some(id) = find(tree, name) else { return Reply::Error(format!("no node `{name}`")) };
    let view = tree.view();
    let Some(node) = find_node(&view, id) else { return Reply::Error(format!("no node `{name}`")) };
    let env = tree.condition_env(id);
    let mut sources = Vec::new();
    let mut declared = std::collections::HashSet::new();
    for set in node.declarations() {
        for (index, declaration) in set.declarations().iter().enumerate() {
            let value = &declaration.declaration.value;
            let written = set.source(index).map_or_else(|| declaration.to_string(), str::to_owned);
            let resolved = theme
                .tokens()
                .resolve(value)
                .map(|resolved| resolved.to_string())
                .filter(|resolved| *resolved != value.to_string());
            let condition = declaration.condition.to_string();
            let applies = declaration.condition.holds_in(&env);
            if applies && declaration.condition.state.is_none() {
                declared.insert(declaration.declaration.property);
            }
            sources.push(StyleSource {
                property: declaration.declaration.property.to_string(),
                value: value.to_string(),
                resolved,
                origin: match set.kind() {
                    SetKind::Classes => Origin::Class { class: written },
                    SetKind::Declarations => Origin::Declaration { text: written },
                },
                condition: (!condition.is_empty()).then_some(condition),
                applies,
            });
        }
    }
    // Every visual property no applying declaration set (in the normal
    // state) comes from the typed override or the theme's default for the
    // kind — the component default.
    let defaults = theme.resolve(node.kind(), ControlState::Normal, &StyleOverride::default());
    // What the application wrote in code, before declarations were folded
    // in: a declaration setting one typography property copies the theme's
    // others, which are the theme's, not a typed override.
    let authored = tree.authored_view();
    let authored = authored.as_ref().and_then(|view| find_node(view, id)).unwrap_or(node);
    for property in VISUAL {
        if declared.contains(&property) {
            continue;
        }
        let (value, origin) = match visual(authored.visual_style(), property) {
            Some(value) => (value, Origin::TypedOverride),
            None => match visual(defaults.properties(), property) {
                Some(value) => {
                    (value, Origin::ComponentDefault { kind: format!("{:?}", node.kind()) })
                }
                None => continue,
            },
        };
        sources.push(StyleSource {
            property: property.to_string(),
            value,
            resolved: None,
            origin,
            condition: None,
            applies: true,
        });
    }
    Reply::of(&StyleExplanation {
        node: node_name(id),
        key: id.local_key(),
        kind: format!("{:?}", node.kind()),
        sources,
    })
}
