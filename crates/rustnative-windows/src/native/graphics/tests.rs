//! Pixel tests for the Direct2D translation.
//!
//! Each test draws a `DrawList` through [`super::d2d::draw`] — the same
//! function the canvas window paints with — into a WIC bitmap render
//! target, then reads the bitmap's actual pixels back. What is asserted is
//! what Direct2D rasterized, not what the translation meant to ask for.

use rustnative_core::{Color, DrawList, ImageData, Paint, Path, RectF, Transform2D, Vec2};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapCacheOnLoad,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

use super::d2d;

const SIZE: u32 = 40;
/// [`SIZE`] in canvas units.
const SIZE_F: f32 = 40.0;

/// A rasterized image: BGRA, premultiplied, `SIZE` pixels square.
struct Pixels(Vec<u8>);

impl Pixels {
    /// `[blue, green, red, alpha]` at `(x, y)`.
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * SIZE + x) * 4) as usize;
        [self.0[start], self.0[start + 1], self.0[start + 2], self.0[start + 3]]
    }
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [0, 0, 255, 255];
const BLUE: [u8; 4] = [255, 0, 0, 255];

fn red() -> Paint {
    Paint::color(Color::rgb(255, 0, 0))
}

/// Draws `list` on a white `SIZE`x`SIZE` bitmap and returns its pixels.
fn rasterize(list: &DrawList) -> Pixels {
    // SAFETY: initializes COM for this test thread; balanced below when it
    // succeeded (it reports `S_FALSE` if already initialized, also a
    // success that must be balanced).
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let pixels = {
        // SAFETY: a documented CLSID and interface; no outer object.
        let wic: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .expect("WIC is part of every supported Windows");
        // SAFETY: plain values and a documented pixel-format GUID.
        let bitmap = unsafe {
            wic.CreateBitmap(SIZE, SIZE, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
        }
        .expect("a small WIC bitmap");
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        d2d::with_factories(|factories| {
            // SAFETY: `bitmap` is live and `properties` borrowed for the call.
            let target = unsafe {
                factories.d2d.CreateWicBitmapRenderTarget(&bitmap, &raw const properties)
            }
            .expect("a WIC render target");
            // SAFETY: `BeginDraw`/`EndDraw` bracket the drawing.
            unsafe {
                target.BeginDraw();
                target.Clear(Some(&d2d::d2d_color(Color::rgb(255, 255, 255))));
            }
            d2d::draw(&target, factories, list).expect("drawing resources");
            // SAFETY: matches the `BeginDraw` above.
            unsafe { target.EndDraw(None, None) }.expect("EndDraw");
        })
        .expect("Direct2D factories");
        let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
        // SAFETY: a null rectangle copies the whole bitmap; the stride and
        // buffer match its size and 32-bit format.
        unsafe { bitmap.CopyPixels(std::ptr::null(), SIZE * 4, &mut pixels) }.expect("CopyPixels");
        Pixels(pixels)
    };
    if initialized {
        // SAFETY: balances the successful `CoInitializeEx` above, after
        // every COM object this function created has been released.
        unsafe { CoUninitialize() };
    }
    pixels
}

fn full() -> RectF {
    RectF::new(0.0, 0.0, SIZE_F, SIZE_F)
}

#[test]
fn a_fill_colors_exactly_the_pixels_it_covers() {
    let pixels = rasterize(&DrawList::new().fill_rect(RectF::new(10.0, 10.0, 20.0, 20.0), red()));
    assert_eq!(pixels.at(20, 20), RED, "inside");
    assert_eq!(pixels.at(5, 5), WHITE, "outside");
    assert_eq!(pixels.at(29, 29), RED, "the last covered pixel");
    assert_eq!(pixels.at(30, 30), WHITE, "the first uncovered one");
}

#[test]
fn an_edge_between_pixels_is_antialiased_to_an_intermediate_value() {
    // The left edge sits halfway across pixel column 10.
    let black = Paint::color(Color::rgb(0, 0, 0));
    let pixels = rasterize(&DrawList::new().fill_rect(RectF::new(10.5, 0.0, 20.0, SIZE_F), black));
    let [blue, green, red, _] = pixels.at(10, 20);
    assert!((64..=192).contains(&red), "half-covered pixel is grey, got {red}");
    assert_eq!((blue, green), (red, red), "and neutral grey, not tinted");
    assert_eq!(pixels.at(9, 20), WHITE);
    assert_eq!(pixels.at(11, 20)[2], 0, "fully covered is black");
}

#[test]
fn a_clip_excludes_what_it_does_not_contain() {
    let list = DrawList::new()
        .push_clip(RectF::new(0.0, 0.0, 10.0, SIZE_F))
        .fill_rect(full(), red())
        .pop();
    let pixels = rasterize(&list);
    assert_eq!(pixels.at(5, 20), RED);
    assert_eq!(pixels.at(20, 20), WHITE, "outside the clip nothing is drawn");
}

#[test]
fn a_rotated_clip_clips_to_the_rotated_shape_not_its_bounds() {
    // A 20x20 square rotated 45 degrees about the canvas center: its
    // corners of the bounding box are outside the diamond.
    let center = SIZE_F / 2.0;
    let turn = Transform2D::translation(-center, -center)
        .then(Transform2D::rotation(std::f32::consts::FRAC_PI_4))
        .then(Transform2D::translation(center, center));
    let nested = DrawList::new()
        .push_transform(turn)
        .push_clip(RectF::new(center - 10.0, center - 10.0, 20.0, 20.0))
        .push_transform(turn.inverse().expect("a rotation is invertible"))
        .fill_rect(full(), red())
        .pop()
        .pop()
        .pop();
    let pixels = rasterize(&nested);
    assert_eq!(pixels.at(20, 20), RED, "the center is inside the diamond");
    assert_eq!(pixels.at(7, 7), WHITE, "a bounding-box corner is outside the diamond");
    assert_eq!(pixels.at(20, 8), RED, "a diamond tip is inside it");
}

#[test]
fn a_transform_moves_what_is_drawn_under_it_and_only_until_popped() {
    let list = DrawList::new()
        .push_transform(Transform2D::translation(20.0, 0.0))
        .fill_rect(RectF::new(0.0, 0.0, 10.0, 10.0), red())
        .pop()
        .fill_rect(RectF::new(0.0, 30.0, 10.0, 10.0), Paint::color(Color::rgb(0, 0, 255)));
    let pixels = rasterize(&list);
    assert_eq!(pixels.at(25, 5), RED, "moved");
    assert_eq!(pixels.at(5, 5), WHITE, "not left behind");
    assert_eq!(pixels.at(5, 35), BLUE, "the transform no longer applies");
}

#[test]
fn an_opacity_layer_blends_its_contents_as_one() {
    let black = Paint::color(Color::rgb(0, 0, 0));
    // Two overlapping black squares at half opacity: as one layer, the
    // overlap is the same grey as the rest, not darker.
    let list = DrawList::new()
        .push_opacity(0.5)
        .fill_rect(RectF::new(0.0, 0.0, 30.0, 30.0), black)
        .fill_rect(RectF::new(10.0, 10.0, 30.0, 30.0), black)
        .pop();
    let pixels = rasterize(&list);
    let single = pixels.at(5, 5)[2];
    let overlap = pixels.at(20, 20)[2];
    assert!((120..=135).contains(&single), "half-transparent black on white is grey, got {single}");
    assert_eq!(single, overlap, "a layer composites once, so overlap is not darker");
}

#[test]
fn paths_fill_their_outline() {
    let triangle = Path::new().move_to(0.0, 0.0).line_to(40.0, 0.0).line_to(0.0, 40.0).close();
    let pixels = rasterize(&DrawList::new().fill_path(triangle, red()));
    assert_eq!(pixels.at(5, 5), RED, "inside the triangle");
    assert_eq!(pixels.at(35, 35), WHITE, "beyond its hypotenuse");
}

#[test]
fn images_are_drawn_scaled_into_their_rectangle() {
    // Two pixels, red then blue, stretched across the whole canvas.
    let image = ImageData::rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255], false).unwrap();
    let pixels = rasterize(&DrawList::new().image(image, full()));
    assert_eq!(pixels.at(2, 20), RED);
    assert_eq!(pixels.at(37, 20), BLUE);
}

#[test]
fn text_is_rasterized_where_it_was_placed() {
    let list =
        DrawList::new().text(Vec2::new(4.0, 4.0), "\u{2588}\u{2588}", 24.0, Color::rgb(0, 0, 0));
    let pixels = rasterize(&list);
    let darkest = (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .map(|(x, y)| pixels.at(x, y)[2])
        .min()
        .unwrap_or(255);
    assert!(darkest < 64, "the glyphs put dark pixels on the canvas (darkest {darkest})");
    assert_eq!(pixels.at(1, 1), WHITE, "and nothing above-left of where the text starts");
}
