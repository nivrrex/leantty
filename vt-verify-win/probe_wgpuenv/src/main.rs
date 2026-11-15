// 探针：wgpu 30.0.2 的 Backends::from_env() 到底读什么、怎么解析。
fn main() {
    let v = std::env::var("WGPU_BACKEND").ok();
    println!("env WGPU_BACKEND={:?}", v);
    println!("Backends::from_env() = {:?}", wgpu::Backends::from_env());
    println!("Backends::all() = {:?}", wgpu::Backends::all());
}
