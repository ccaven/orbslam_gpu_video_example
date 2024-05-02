use std::borrow::Cow;

use tiny_wgpu::{
    Compute, Storage, ComputeProgram, ComputeKernel, BindGroupItem
};

struct Decoder<'a> {
    compute: &'a Compute,
    storage: Storage,
    image_size: wgpu::Extent3d
}

impl ComputeProgram for Decoder<'_> {
    fn storage(&self) -> &Storage {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut Storage {
        &mut self.storage
    }

    fn compute(&self) -> &Compute {
        self.compute
    }
}

impl Decoder<'_> {

    pub fn init(&mut self) {
        
        let shared = include_str!("shaders/decoder/shared.wgsl");
        let huffman = include_str!("shaders/decoder/huffman.wgsl");
        let dct = include_str!("shaders/decoder/dct.wgsl");
        let huffman = format!("{shared}\n\n{huffman}");
        let dct = format!("{shared}\n\n{dct}");
        
        self.add_module("huffman", wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(&huffman))
        });

        self.add_module("dct", wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(&dct))
        });

        

    }

}