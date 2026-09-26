//! Turning a portable [`DrawList`] into Direct2D calls.
//!
//! This is the *only* translation: the canvas window draws through it into
//! its `ID2D1HwndRenderTarget`, and the pixel tests draw through it into a
//! WIC bitmap and read the pixels back. A test that passed through a second
//! copy of this logic would be testing the copy.
//!
//! # Scopes
//!
//! `PushTransform` composes onto the current transform. `PushClip` and
//! `PushOpacity` are both Direct2D *layers*: a clip is a layer with a
//! rectangular geometric mask under the transform current when it was
//! pushed, which — unlike `PushAxisAlignedClip`, which widens a rotated
//! clip to its bounding box — clips exactly what
//! [`DrawList::hit_test`] says it clips. Scopes a list leaves open are
//! closed here, so a malformed list cannot leak state into the next frame.

use std::cell::OnceCell;
use std::mem::ManuallyDrop;

use rustnative_core::{
    Color, DrawCommand, DrawList, Paint, Path, PathSegment, RectF, Transform2D, Vec2,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS,
    D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_ROUNDED_RECT, D2D1CreateFactory, ID2D1Factory,
    ID2D1Geometry, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::{Interface, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

/// The process's Direct2D and DirectWrite factories, one pair per UI
/// thread.
///
/// Both are plain factories rather than apartment-bound COM objects
/// (neither is created through `CoCreateInstance`), so holding them in a
/// thread-local that is released at thread exit is safe — unlike the
/// `IAccPropServices` lesson from Milestone 26.
pub(crate) struct Factories {
    pub(crate) d2d: ID2D1Factory,
    pub(crate) dwrite: IDWriteFactory,
}

thread_local! {
    static FACTORIES: OnceCell<Option<Factories>> = const { OnceCell::new() };
}

/// Runs `f` with this thread's factories, creating them on first use.
///
/// `None` if either could not be created, which on a supported Windows
/// version means the graphics stack itself is unavailable (a session
/// without a display driver); a canvas then paints nothing rather than
/// taking the application down.
pub(crate) fn with_factories<R>(f: impl FnOnce(&Factories) -> R) -> Option<R> {
    FACTORIES.with(|cell| {
        cell.get_or_init(|| {
            // SAFETY: both calls take plain enum arguments and return an
            // owned interface or an error; no pointers are involved.
            let d2d = unsafe {
                D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
            }
            .ok()?;
            // SAFETY: as above.
            let dwrite =
                unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }
                    .ok()?;
            Some(Factories { d2d, dwrite })
        })
        .as_ref()
        .map(f)
    })
}

/// What a `Push*` opened, so `Pop` knows what to undo.
#[derive(Clone, Copy)]
enum Scope {
    Transform,
    Layer,
}

/// Draws `list` into `target`, which must be between `BeginDraw` and
/// `EndDraw`.
///
/// # Errors
///
/// A Direct2D or DirectWrite resource that could not be created. Drawing
/// calls themselves report failure only at `EndDraw`, which is the caller's.
pub(crate) fn draw(
    target: &ID2D1RenderTarget,
    factories: &Factories,
    list: &DrawList,
) -> Result<()> {
    let mut transforms = vec![Transform2D::identity()];
    let mut scopes: Vec<Scope> = Vec::new();
    // SAFETY: `target` is a live render target in a drawing state (the
    // caller's contract); `matrix` is a valid, borrowed `Matrix3x2`.
    unsafe { target.SetTransform(&matrix(Transform2D::identity())) };

    for command in list.commands() {
        let current = *transforms.last().unwrap_or(&Transform2D::identity());
        match command {
            DrawCommand::FillRect(rect, paint) => {
                let brush = brush(target, paint.color)?;
                // SAFETY: `rect`/`brush` are live and borrowed for the call.
                unsafe { target.FillRectangle(&d2d_rect(*rect), &brush) };
            }
            DrawCommand::StrokeRect(rect, paint) => {
                let brush = brush(target, paint.color)?;
                // SAFETY: as above; a `None` stroke style is the documented
                // default solid stroke.
                unsafe {
                    target.DrawRectangle(&d2d_rect(*rect), &brush, paint.stroke_width.get(), None);
                }
            }
            DrawCommand::FillRoundedRect(rect, radius, paint) => {
                let brush = brush(target, paint.color)?;
                let rounded = D2D1_ROUNDED_RECT {
                    rect: d2d_rect(*rect),
                    radiusX: radius.get(),
                    radiusY: radius.get(),
                };
                // SAFETY: as above.
                unsafe { target.FillRoundedRectangle(&raw const rounded, &brush) };
            }
            DrawCommand::FillEllipse(rect, paint) => {
                let brush = brush(target, paint.color)?;
                let ellipse = D2D1_ELLIPSE {
                    point: Vector2 {
                        X: rect.x.get() + rect.width.get() / 2.0,
                        Y: rect.y.get() + rect.height.get() / 2.0,
                    },
                    radiusX: rect.width.get() / 2.0,
                    radiusY: rect.height.get() / 2.0,
                };
                // SAFETY: as above.
                unsafe { target.FillEllipse(&raw const ellipse, &brush) };
            }
            DrawCommand::StrokeLine(from, to, paint) => {
                let brush = brush(target, paint.color)?;
                // SAFETY: as above.
                unsafe {
                    target.DrawLine(
                        vector(*from),
                        vector(*to),
                        &brush,
                        paint.stroke_width.get(),
                        None,
                    );
                }
            }
            DrawCommand::FillPath(path, paint) => {
                let geometry = geometry(&factories.d2d, path)?;
                let brush = brush(target, paint.color)?;
                // SAFETY: as above; a `None` opacity brush means none.
                unsafe { target.FillGeometry(&geometry, &brush, None) };
            }
            DrawCommand::StrokePath(path, paint) => {
                stroke_path(target, &factories.d2d, path, *paint)?;
            }
            DrawCommand::Text { origin, text, size, color } => {
                draw_text(target, &factories.dwrite, *origin, text, size.get(), *color)?;
            }
            DrawCommand::Image(image, rect) => {
                let pixels = image.premultiplied_bgra();
                let properties = D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                };
                // SAFETY: `pixels` is exactly `width * height * 4` bytes
                // (`ImageData` validated its dimensions), so a pitch of
                // `width * 4` reads inside it; the copy is made before this
                // call returns.
                let bitmap = unsafe {
                    target.CreateBitmap(
                        D2D_SIZE_U { width: image.width(), height: image.height() },
                        Some(pixels.as_ptr().cast()),
                        image.width().saturating_mul(4),
                        &raw const properties,
                    )
                }?;
                // SAFETY: `bitmap` was just created on this target.
                unsafe {
                    target.DrawBitmap(
                        &bitmap,
                        Some(&d2d_rect(*rect)),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
            }
            DrawCommand::PushTransform(transform) => {
                let next = transform.then(current);
                transforms.push(next);
                scopes.push(Scope::Transform);
                // SAFETY: as for the initial `SetTransform`.
                unsafe { target.SetTransform(&matrix(next)) };
            }
            DrawCommand::PushClip(rect) => {
                // SAFETY: `rect` is a plain value; the factory is live.
                let mask: ID2D1Geometry =
                    unsafe { factories.d2d.CreateRectangleGeometry(&d2d_rect(*rect)) }?.cast()?;
                push_layer(target, Some(mask), 1.0)?;
                scopes.push(Scope::Layer);
            }
            DrawCommand::PushOpacity(opacity) => {
                push_layer(target, None, opacity.get())?;
                scopes.push(Scope::Layer);
            }
            DrawCommand::Pop => pop(target, &mut scopes, &mut transforms),
            // Declares where input goes; draws nothing.
            DrawCommand::HitRegion(..) => {}
        }
    }
    while !scopes.is_empty() {
        pop(target, &mut scopes, &mut transforms);
    }
    Ok(())
}

/// Closes the innermost scope.
fn pop(target: &ID2D1RenderTarget, scopes: &mut Vec<Scope>, transforms: &mut Vec<Transform2D>) {
    match scopes.pop() {
        Some(Scope::Transform) => {
            transforms.pop();
            let current = *transforms.last().unwrap_or(&Transform2D::identity());
            // SAFETY: as for the initial `SetTransform`.
            unsafe { target.SetTransform(&matrix(current)) };
        }
        // SAFETY: a `Layer` scope is only recorded after a successful
        // `PushLayer`, so there is a layer to pop.
        Some(Scope::Layer) => unsafe { target.PopLayer() },
        // A `Pop` with nothing open is ignored, as the draw list documents.
        None => {}
    }
}

/// Pushes a layer: clipped to `mask` if one is given, drawn at `opacity`.
///
/// The mask is declared in the space current at the push. Direct2D already
/// carries it through the world transform in force when the layer is
/// pushed, so `maskTransform` stays identity: passing the current transform
/// there as well applied it twice, which the rotated-clip pixel test caught
/// as a square turned 90 degrees instead of 45.
fn push_layer(target: &ID2D1RenderTarget, mask: Option<ID2D1Geometry>, opacity: f32) -> Result<()> {
    // SAFETY: a `None` size asks for a layer sized to the target, the
    // documented default.
    let layer = unsafe { target.CreateLayer(None) }?;
    let parameters = D2D1_LAYER_PARAMETERS {
        // The whole plane: the mask, not these bounds, is what clips.
        contentBounds: D2D_RECT_F {
            left: -f32::MAX,
            top: -f32::MAX,
            right: f32::MAX,
            bottom: f32::MAX,
        },
        geometricMask: ManuallyDrop::new(mask),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        maskTransform: matrix(Transform2D::identity()),
        opacity,
        opacityBrush: ManuallyDrop::new(None),
        layerOptions: D2D1_LAYER_OPTIONS_NONE,
    };
    // SAFETY: `parameters` and `layer` are live and borrowed for the call;
    // Direct2D takes its own references to the mask it keeps.
    unsafe { target.PushLayer(&raw const parameters, &layer) };
    // The `ManuallyDrop` fields are this function's to release once the
    // target has taken its own reference.
    drop(ManuallyDrop::into_inner(parameters.geometricMask));
    drop(ManuallyDrop::into_inner(parameters.opacityBrush));
    Ok(())
}

fn stroke_path(
    target: &ID2D1RenderTarget,
    factory: &ID2D1Factory,
    path: &Path,
    paint: Paint,
) -> Result<()> {
    let geometry = geometry(factory, path)?;
    let brush = brush(target, paint.color)?;
    // SAFETY: `geometry`/`brush` are live and borrowed for the call.
    unsafe { target.DrawGeometry(&geometry, &brush, paint.stroke_width.get(), None) };
    Ok(())
}

fn draw_text(
    target: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    origin: Vec2,
    text: &str,
    size: f32,
    color: Color,
) -> Result<()> {
    if size <= 0.0 || text.is_empty() {
        return Ok(());
    }
    // SAFETY: the family and locale are static wide literals; a `None` font
    // collection is the documented "system fonts".
    let format = unsafe {
        dwrite.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-us"),
        )
    }?;
    let brush = brush(target, color)?;
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    // An unbounded layout box: the text is one line starting at `origin`,
    // not wrapped to anything.
    let layout =
        D2D_RECT_F { left: origin.x.get(), top: origin.y.get(), right: f32::MAX, bottom: f32::MAX };
    // SAFETY: `utf16`, `format`, `layout`, and `brush` are live and
    // borrowed for the call.
    unsafe {
        target.DrawText(
            &utf16,
            &format,
            &raw const layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
    Ok(())
}

/// Builds a path geometry from a portable path.
fn geometry(factory: &ID2D1Factory, path: &Path) -> Result<ID2D1PathGeometry> {
    // SAFETY: the factory is live and `CreatePathGeometry` takes no
    // arguments.
    let geometry = unsafe { factory.CreatePathGeometry() }?;
    // SAFETY: `geometry` was just created and has never been opened.
    let sink = unsafe { geometry.Open() }?;
    // SAFETY: `sink` stays open for the whole block (it is closed at its
    // end, on the only way out), every call takes plain values, and figures
    // are begun before segments are added and ended before `Close` — the
    // order the geometry-sink contract requires.
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        let mut open = false;
        for segment in path.segments() {
            match *segment {
                PathSegment::MoveTo(point) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(vector(point), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                }
                PathSegment::Close => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                }
                PathSegment::LineTo(to)
                | PathSegment::QuadTo { to, .. }
                | PathSegment::CubicTo { to, .. } => {
                    // A segment before any `MoveTo` starts its figure at the
                    // origin, as the HTML canvas does.
                    if !open {
                        sink.BeginFigure(Vector2 { X: 0.0, Y: 0.0 }, D2D1_FIGURE_BEGIN_FILLED);
                        open = true;
                    }
                    match *segment {
                        PathSegment::QuadTo { control, .. } => {
                            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                                point1: vector(control),
                                point2: vector(to),
                            });
                        }
                        PathSegment::CubicTo { first, second, .. } => {
                            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                                point1: vector(first),
                                point2: vector(second),
                                point3: vector(to),
                            });
                        }
                        _ => sink.AddLine(vector(to)),
                    }
                }
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
    }
    Ok(geometry)
}

fn brush(target: &ID2D1RenderTarget, color: Color) -> Result<ID2D1SolidColorBrush> {
    // SAFETY: `color` is a plain value borrowed for the call; a `None`
    // brush-properties pointer is the documented default.
    unsafe { target.CreateSolidColorBrush(&d2d_color(color), None) }
}

pub(crate) fn d2d_color(color: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.red) / 255.0,
        g: f32::from(color.green) / 255.0,
        b: f32::from(color.blue) / 255.0,
        a: f32::from(color.alpha) / 255.0,
    }
}

fn d2d_rect(rect: RectF) -> D2D_RECT_F {
    D2D_RECT_F { left: rect.x.get(), top: rect.y.get(), right: rect.right(), bottom: rect.bottom() }
}

fn vector(point: Vec2) -> Vector2 {
    Vector2 { X: point.x.get(), Y: point.y.get() }
}

fn matrix(transform: Transform2D) -> Matrix3x2 {
    Matrix3x2 {
        M11: transform.m11.get(),
        M12: transform.m12.get(),
        M21: transform.m21.get(),
        M22: transform.m22.get(),
        M31: transform.dx.get(),
        M32: transform.dy.get(),
    }
}
