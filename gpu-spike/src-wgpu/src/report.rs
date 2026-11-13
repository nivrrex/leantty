//! 测量报告输出（人类可读 + 单行 JSON，供脚本聚合）。

use crate::gpu::Info;
use spike_common::Stats;

pub struct InitReport {
    pub instance_adapter_device_ms: f64,
    pub atlas_ms: f64,
    pub pipeline_ms: f64,
}

pub struct RunReport {
    pub gpu: Info,
    pub cols: u16,
    pub rows: u16,
    pub scale: f64,
    pub pixel: (u32, u32),
    pub cell: (u32, u32),
    pub init: InitReport,
    pub frames: u32,
    pub build: Stats,
    pub submit: Stats,
    pub readback: Stats,
    pub diff: spike_common::DiffReport,
    pub cpu_render: Stats,
}

impl RunReport {
    pub fn print(&self) {
        println!("== spike-wgpu (Vulkan) ==");
        println!(
            "gpu={} backend={:?} type={:?} driver={} ({}) api={}",
            self.gpu.name,
            self.gpu.backend,
            self.gpu.device_type,
            self.gpu.driver,
            self.gpu.driver_info,
            self.gpu.api_version
        );
        println!(
            "size={}x{} scale={} pixel={}x{} cell={}x{} frames={}",
            self.cols, self.rows, self.scale, self.pixel.0, self.pixel.1, self.cell.0, self.cell.1, self.frames
        );
        println!(
            "init: adapter+device={:.3}ms atlas={:.3}ms pipeline+resources={:.3}ms total={:.3}ms",
            self.init.instance_adapter_device_ms,
            self.init.atlas_ms,
            self.init.pipeline_ms,
            self.init.instance_adapter_device_ms + self.init.atlas_ms + self.init.pipeline_ms
        );
        println!(
            "per-frame: build med={:.3} p95={:.3} | submit+wait med={:.3} p95={:.3} | readback med={:.3} p95={:.3} (ms)",
            self.build.median_ms,
            self.build.p95_ms,
            self.submit.median_ms,
            self.submit.p95_ms,
            self.readback.median_ms,
            self.readback.p95_ms
        );
        println!(
            "cpu-render (同快照同尺寸, {} 帧): med={:.3} p95={:.3} ms",
            self.frames.min(60),
            self.cpu_render.median_ms,
            self.cpu_render.p95_ms
        );
        println!(
            "pixel-diff vs CPU: total={} diff={} (bg={} glyph={}/{} glyph_total) max_channel={} glyph_max_channel={}",
            self.diff.total,
            self.diff.diff_pixels,
            self.diff.bg_diff_pixels,
            self.diff.glyph_diff_pixels,
            self.diff.glyph_pixels,
            self.diff.max_channel_diff,
            self.diff.glyph_max_channel_diff
        );
    }

    /// 单行 JSON（脚本聚合用；字段名固定）。
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{\"impl\":\"wgpu\",\"gpu\":\"{}\",\"backend\":\"{:?}\",\"device_type\":\"{:?}\",",
                "\"driver\":\"{}\",\"api\":\"{}\",\"cols\":{},\"rows\":{},\"scale\":{},",
                "\"pw\":{},\"ph\":{},\"cw\":{},\"ch\":{},\"frames\":{},",
                "\"init_adapter_device_ms\":{:.3},\"init_atlas_ms\":{:.3},\"init_pipeline_ms\":{:.3},",
                "\"build_med\":{:.3},\"build_p95\":{:.3},",
                "\"submit_med\":{:.3},\"submit_p95\":{:.3},",
                "\"readback_med\":{:.3},\"readback_p95\":{:.3},",
                "\"cpu_med\":{:.3},\"cpu_p95\":{:.3},",
                "\"diff_pixels\":{},\"bg_diff\":{},\"glyph_diff\":{},\"glyph_total\":{},",
                "\"max_channel_diff\":{},\"glyph_max_channel_diff\":{}}}"
            ),
            self.gpu.name,
            self.gpu.backend,
            self.gpu.device_type,
            self.gpu.driver,
            self.gpu.api_version,
            self.cols,
            self.rows,
            self.scale,
            self.pixel.0,
            self.pixel.1,
            self.cell.0,
            self.cell.1,
            self.frames,
            self.init.instance_adapter_device_ms,
            self.init.atlas_ms,
            self.init.pipeline_ms,
            self.build.median_ms,
            self.build.p95_ms,
            self.submit.median_ms,
            self.submit.p95_ms,
            self.readback.median_ms,
            self.readback.p95_ms,
            self.cpu_render.median_ms,
            self.cpu_render.p95_ms,
            self.diff.diff_pixels,
            self.diff.bg_diff_pixels,
            self.diff.glyph_diff_pixels,
            self.diff.glyph_pixels,
            self.diff.max_channel_diff,
            self.diff.glyph_max_channel_diff
        )
    }
}
