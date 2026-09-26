//! Semantic conventions: the attribute names every span of a kind uses,
//! so a dashboard reads a render, a task, a service call, a request, or a
//! job from any application the same way. HTTP names follow OpenTelemetry's.

/// The request method.
pub const HTTP_METHOD: &str = "http.request.method";
/// The response status.
pub const HTTP_STATUS: &str = "http.response.status_code";
/// The full URL of an outgoing request.
pub const URL_FULL: &str = "url.full";
/// The path of an incoming request.
pub const URL_PATH: &str = "url.path";
/// The route pattern that answered.
pub const HTTP_ROUTE: &str = "http.route";
/// The component a render span rendered.
pub const COMPONENT: &str = "rustnative.component";
/// Why it rendered (or skipped).
pub const RENDER_REASON: &str = "rustnative.render.reason";
/// A task's owner.
pub const TASK_OWNER: &str = "rustnative.task.owner";
/// A service call's service.
pub const SERVICE: &str = "rustnative.service";
/// A job's kind.
pub const JOB_KIND: &str = "rustnative.job.kind";
/// A job's attempt.
pub const JOB_ATTEMPT: &str = "rustnative.job.attempt";

/// The path of a URL (for span names, which must not carry query strings).
#[must_use]
pub fn path_of(url: &str) -> &str {
    let after_scheme = url.find("://").map_or(url, |at| &url[at + 3..]);
    let path = after_scheme.find('/').map_or("/", |at| &after_scheme[at..]);
    path.split(['?', '#']).next().unwrap_or("/")
}
