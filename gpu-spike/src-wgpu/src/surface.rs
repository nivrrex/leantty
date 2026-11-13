//! T4b：在 DISPLAY=:99 下用 lavapipe 创建 X11 surface 并 present 若干帧。
//!
//! 只允许 :99（与 e2e 的 guard.sh 约定一致）；若 DISPLAY 不是 :99 直接拒绝，
//! 避免误触用户桌面 :0。

use std::error::Error;
use std::ffi::c_void;
use std::ptr;
use std::time::Instant;

use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, XlibDisplayHandle, XlibWindowHandle,
};

pub fn run_surface(frames: u32) -> Result<(), Box<dyn Error>> {
    let display_env = std::env::var("DISPLAY").unwrap_or_default();
    if !display_env.ends_with(":99") && !display_env.ends_with(":99.0") {
        return Err(format!(
            "拒绝在 DISPLAY='{display_env}' 上创建 surface（只允许 :99）"
        )
        .into());
    }
    let icd = std::env::var("VK_ICD_FILENAMES").unwrap_or_default();
    eprintln!("[surface] DISPLAY={display_env} VK_ICD_FILENAMES={icd}");

    // 打开 :99 的 X 连接并创建窗口（XCreateSimpleWindow + map）。
    let xlib = x11_dl::xlib::Xlib::open()?;
    // SAFETY: Xlib 全局初始化与显示连接为 C API 的标准调用序列。
    let (display, window, screen) = unsafe {
        let d = (xlib.XOpenDisplay)(ptr::null());
        if d.is_null() {
            return Err(format!("XOpenDisplay 失败（DISPLAY={display_env}）").into());
        }
        let screen = (xlib.XDefaultScreen)(d);
        let root = (xlib.XRootWindow)(d, screen);
        let win = (xlib.XCreateSimpleWindow)(d, root, 0, 0, 480, 240, 0, 0, 0);
        (xlib.XMapWindow)(d, win);
        (xlib.XFlush)(d);
        (d, win, screen)
    };
    // 等窗口真正可映射（X11 异步：短暂忙等避免 surface 配置失败）。
    unsafe { (xlib.XSync)(display, 0) };

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::empty(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });
    let t = Instant::now();
    // SAFETY: display/window 均为本进程打开的 Xlib 句柄，存活到 surface 销毁之前。
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                // SAFETY: display 非空（上面已判空）。
                Some(std::ptr::NonNull::new_unchecked(display.cast::<c_void>())),
                screen,
            ))),
            raw_window_handle: RawWindowHandle::Xlib(XlibWindowHandle::new(window)),
        })
    };
    // 上面 XlibDisplayHandle::new 需要 NonNull<c_void>：用 raw 指针直接构造。
    let surface = match surface {
        Ok(s) => s,
        Err(e) => {
            return Err(format!("create_surface 失败: {e}").into());
        }
    };
    let surface_ms = t.elapsed().as_secs_f64() * 1000.0;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
        apply_limit_buckets: false,
    }))?;
    let info = adapter.get_info();
    eprintln!(
        "[surface] adapter='{}' type={:?}（lavapipe 应报 Cpu）",
        info.name, info.device_type
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("surface-spike"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))?;

    let caps = surface.get_capabilities(&adapter);
    let format = caps.formats.first().copied().unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
    let (w, h) = (480u32, 240u32);
    surface.configure(
        &device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            // 默认 Auto：由后端按格式选（不引入 HDR 相关行为）。
            color_space: wgpu::SurfaceColorSpace::Auto,
        },
    );

    let mut ok = 0u32;
    let mut last_err = String::new();
    for i in 0..frames {
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                last_err = format!("{other:?}");
                break;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("surface"),
        });
        {
            let t = i as f64 / frames.max(1) as f64;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: t,
                            g: 0.2,
                            b: 1.0 - t,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let _ = &mut pass;
        }
        queue.submit([enc.finish()]);
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;
        queue.present(frame);
        ok += 1;
    }
    eprintln!(
        "[surface] present 成功 {ok}/{frames} 帧，format={format:?}，surface 创建 {surface_ms:.1}ms{}",
        if last_err.is_empty() {
            String::new()
        } else {
            format!("，最后一次错误: {last_err}")
        }
    );
    // SAFETY: 关闭自己创建的 X 连接。
    unsafe {
        (xlib.XDestroyWindow)(display, window);
        (xlib.XCloseDisplay)(display);
    }
    if ok == 0 {
        return Err("surface present 全部失败".into());
    }
    Ok(())
}

#[allow(dead_code)]
fn unused(_: *mut c_void) {}
