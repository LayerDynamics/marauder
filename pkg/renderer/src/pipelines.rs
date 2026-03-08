//! wgpu render pipelines for background, text, cursor, and overlay passes.

use wgpu;

use crate::images::ImageInstance;
use crate::types::{BgInstance, ComputeOverlayInstance, SelectionInstance, TextInstance};

/// Maximum allowed WGSL source size (64 KiB).
const MAX_WGSL_SIZE: usize = 64 * 1024;

/// Create a custom overlay render pipeline from trusted WGSL source.
///
/// The shader must define `vs_main` and `fs_main` entry points.
/// Uses alpha blending and the provided bind group layout.
///
/// Validates the WGSL with naga before compiling to catch errors early
/// and prevent malformed shaders from reaching the GPU driver.
pub fn create_overlay_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    wgsl_source: &str,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> Result<wgpu::RenderPipeline, String> {
    // Size limit to prevent resource exhaustion during parsing/compilation
    if wgsl_source.len() > MAX_WGSL_SIZE {
        return Err(format!(
            "WGSL source exceeds maximum size ({} > {MAX_WGSL_SIZE} bytes)",
            wgsl_source.len()
        ));
    }

    // Pre-validate WGSL with naga before handing to the GPU driver
    let module = naga::front::wgsl::parse_str(wgsl_source)
        .map_err(|e| format!("WGSL parse error: {e}"))?;

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    );
    validator.validate(&module)
        .map_err(|e| format!("WGSL validation error: {e}"))?;

    // Verify required entry points exist
    let has_vs = module.entry_points.iter().any(|ep| ep.name == "vs_main");
    let has_fs = module.entry_points.iter().any(|ep| ep.name == "fs_main");
    if !has_vs || !has_fs {
        return Err(format!(
            "WGSL shader must define vs_main and fs_main entry points (found vs_main={has_vs}, fs_main={has_fs})"
        ));
    }

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("overlay_custom_shader"),
        source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("overlay_custom_pipeline_layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    Ok(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("overlay_custom_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    }))
}

/// Create the background render pipeline.
pub fn create_background_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("background_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/background.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("background_pipeline_layout"),
        bind_group_layouts: &[uniform_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("background_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<BgInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // pos
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // size
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    // bg_color
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Create the text render pipeline.
pub fn create_text_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    text_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("text_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/text.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("text_pipeline_layout"),
        bind_group_layouts: &[text_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("text_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TextInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // pos
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // size
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    // fg_color
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                    // uv_rect
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 32,
                        shader_location: 3,
                    },
                    // glyph_offset
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 48,
                        shader_location: 4,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Create the subpixel text render pipeline (uses RGBA atlas for per-subpixel coverage).
pub fn create_subpixel_text_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    text_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("text_subpixel_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/text_subpixel.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("text_subpixel_pipeline_layout"),
        bind_group_layouts: &[text_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("text_subpixel_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TextInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 2 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 3 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 48, shader_location: 4 },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // Subpixel AA requires per-channel blending:
                // - Source RGB is premultiplied (fg_color * coverage), so use One.
                // - Destination RGB is attenuated per-channel by (1 - coverage),
                //   achieved via OneMinusSrc since src.rgb = fg * coverage.
                //   We use OneMinusSrcColor so each R/G/B channel blends
                //   independently by its own subpixel coverage value.
                // - Alpha uses standard premultiplied blending.
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrc,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Create the compute overlay pipeline (search highlights, URL underlines, outlines).
pub fn create_compute_overlay_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("compute_overlay_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/overlay.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("compute_overlay_pipeline_layout"),
        bind_group_layouts: &[uniform_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("compute_overlay_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ComputeOverlayInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // pos
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // size
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    // color
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                    // flags
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 32,
                        shader_location: 3,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Create the selection overlay pipeline (alpha-blended instanced quads).
pub fn create_selection_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("selection_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/selection.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("selection_pipeline_layout"),
        bind_group_layouts: &[uniform_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("selection_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SelectionInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // pos
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // size
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    // color
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Create the image render pipeline (inline images: Sixel, iTerm2).
///
/// Uses two bind group layouts:
/// - group(0): uniform buffer (viewport size, etc.)
/// - group(1): per-image texture + sampler (from `ImageManager::bind_group_layout()`)
pub fn create_image_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bind_group_layout: &wgpu::BindGroupLayout,
    image_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("image_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/image.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("image_pipeline_layout"),
        bind_group_layouts: &[uniform_bind_group_layout, image_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("image_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ImageInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // pos
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // size
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    // uv_rect
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Create the cursor overlay pipeline.
pub fn create_cursor_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    cursor_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("cursor_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../resources/shaders/cursor.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("cursor_pipeline_layout"),
        bind_group_layouts: &[cursor_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cursor_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}
