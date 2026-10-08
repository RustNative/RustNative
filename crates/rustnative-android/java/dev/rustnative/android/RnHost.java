package dev.rustnative.android;

import android.Manifest;
import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageManager;
import android.graphics.SurfaceTexture;
import android.hardware.camera2.CameraAccessException;
import android.hardware.camera2.CameraCaptureSession;
import android.hardware.camera2.CameraDevice;
import android.hardware.camera2.CameraManager;
import android.hardware.camera2.CaptureRequest;
import android.net.Uri;
import android.view.Surface;
import android.view.TextureView;
import android.view.View;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.MediaController;
import android.widget.VideoView;
import java.util.Collections;

/**
 * Host content (`foreign.rs`): what the host draws for the framework —
 * a web page ({@link WebView}), media with the host's controls
 * ({@link VideoView}), a camera's live preview (Camera2 into a
 * {@link TextureView}) — and foreign views created by class name.
 */
final class RnHost {
    private RnHost() {}

    /** A web view showing {@code url}. */
    static View web(Activity activity, String url) {
        WebView web = new WebView(activity);
        web.setWebViewClient(new WebViewClient());
        web.loadUrl(url);
        return web;
    }

    /** The page title a web view shows (null before it loads). */
    static String webTitle(View view) {
        return view instanceof WebView ? ((WebView) view).getTitle() : null;
    }

    /** Whether a web view has finished loading. */
    static int webProgress(View view) {
        return view instanceof WebView ? ((WebView) view).getProgress() : -1;
    }

    /** A video view playing {@code source} (a path or URI), with controls. */
    static View media(Activity activity, String source) {
        VideoView video = new VideoView(activity);
        MediaController controls = new MediaController(activity);
        controls.setAnchorView(video);
        video.setMediaController(controls);
        Uri uri = source.contains("://") ? Uri.parse(source) : Uri.fromFile(new java.io.File(source));
        video.setVideoURI(uri);
        return video;
    }

    /** Whether the application may use the camera now. */
    static boolean cameraAllowed(Context context) {
        return context.getPackageManager().hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)
            && context.checkSelfPermission(Manifest.permission.CAMERA)
                == PackageManager.PERMISSION_GRANTED;
    }

    /** A live preview of camera {@code device} (by the host's index). */
    static View camera(final Activity activity, final int device) {
        final TextureView preview = new TextureView(activity);
        final CameraDevice[] opened = new CameraDevice[1];
        preview.setSurfaceTextureListener(new TextureView.SurfaceTextureListener() {
            @Override
            public void onSurfaceTextureAvailable(SurfaceTexture texture, int width, int height) {
                open(activity, device, texture, opened);
            }

            @Override
            public void onSurfaceTextureSizeChanged(SurfaceTexture texture, int width, int height) {}

            @Override
            public boolean onSurfaceTextureDestroyed(SurfaceTexture texture) {
                if (opened[0] != null) {
                    opened[0].close();
                    opened[0] = null;
                }
                return true;
            }

            @Override
            public void onSurfaceTextureUpdated(SurfaceTexture texture) {}
        });
        return preview;
    }

    private static void open(Activity activity, int device, final SurfaceTexture texture,
        final CameraDevice[] opened) {
        if (!cameraAllowed(activity)) {
            return;
        }
        CameraManager manager = (CameraManager) activity.getSystemService(Context.CAMERA_SERVICE);
        try {
            String[] ids = manager.getCameraIdList();
            if (device < 0 || device >= ids.length) {
                return;
            }
            manager.openCamera(ids[device], new CameraDevice.StateCallback() {
                @Override
                public void onOpened(final CameraDevice camera) {
                    opened[0] = camera;
                    try {
                        final Surface surface = new Surface(texture);
                        final CaptureRequest.Builder request =
                            camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW);
                        request.addTarget(surface);
                        camera.createCaptureSession(Collections.singletonList(surface),
                            new CameraCaptureSession.StateCallback() {
                                @Override
                                public void onConfigured(CameraCaptureSession session) {
                                    try {
                                        session.setRepeatingRequest(request.build(), null, null);
                                    } catch (CameraAccessException ignored) {
                                        camera.close();
                                    }
                                }

                                @Override
                                public void onConfigureFailed(CameraCaptureSession session) {
                                    camera.close();
                                }
                            }, null);
                    } catch (CameraAccessException failed) {
                        camera.close();
                    }
                }

                @Override
                public void onDisconnected(CameraDevice camera) {
                    camera.close();
                }

                @Override
                public void onError(CameraDevice camera, int error) {
                    camera.close();
                }
            }, null);
        } catch (CameraAccessException | SecurityException unavailable) {
            // No camera now: the preview stays empty.
        }
    }

    /**
     * A foreign view of {@code className}, made with its
     * {@code (Context)} constructor, or null when there is no such class.
     */
    static View byClass(Activity activity, String className) {
        try {
            Class<?> type = Class.forName(className, true, activity.getClassLoader());
            Object made = type.getConstructor(Context.class).newInstance(activity);
            return made instanceof View ? (View) made : null;
        } catch (ReflectiveOperationException | RuntimeException missing) {
            return null;
        }
    }

    /** Tags a foreign view so it reports input and hit-tests like the rest. */
    static void adopt(View view, long window, int tag) {
        if (view.getTag() == null) {
            view.setTag(new RnViews.Tag(window, tag, Rn.FOREIGN));
        }
    }
}
