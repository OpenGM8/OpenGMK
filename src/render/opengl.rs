//! OpenGL bindings & functions
//!
//! The raw bindings are generated at build time, see build.rs

/// Auto-generated OpenGL bindings from gl_generator
#[allow(clippy::all)]
mod gl {
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

use crate::{
    atlas::{AtlasBuilder, AtlasRef},
    render::{Renderer, RendererOptions, Texture},
};
use glfw::Context;
use rect_packer::DensePacker;
use std::{
    fs,
    io::{self, BufWriter},
    ops::Drop,
    path::PathBuf,
    ptr,
};

// OpenGL typedefs
use gl::types::{GLint, GLuint};

pub struct OpenGLRenderer {
    window: glfw::Window,

    // -- TEXTURE ATLASES --
    /// Whether the initial atlases have been uploaded (see upload_atlases).
    atlases_initialized: bool,
    /// Atlases' rectangle packers to be reused for dynamic sprite loading.
    atlas_packers: Vec<DensePacker>,
    /// Atlas references (xywh + idx) to be indexed by `Texture`s.
    atlas_refs: Vec<AtlasRef>,
    /// OpenGL's texture handles in identical order to the atlases.
    texture_ids: Vec<GLuint>,
}

impl OpenGLRenderer {
    pub fn new(options: RendererOptions, mut window: glfw::Window) -> Result<Self, String> {
        window.set_icon_from_pixels(options.icons.iter().map(|x| glfw::PixelImage {
            width: x.1,
            height: x.2,
            pixels: x.0
                .rchunks_exact(x.1 as usize * 4)
                .flat_map(|x| x
                    .chunks_exact(4)
                    .map(|r| u32::from_le_bytes([r[2], r[1], r[0], r[3]]))
                ).collect::<Vec<_>>(),
        }).collect());

        window.set_key_polling(true);
        window.set_framebuffer_size_polling(true);

        gl::load_with(|symbol| window.get_proc_address(symbol) as *const _);

        let mut render_context = window.render_context();
        render_context.make_current();

        Ok(Self {
            window,

            atlases_initialized: false,
            atlas_packers: Vec::new(),
            atlas_refs: Vec::new(),
            texture_ids: Vec::new(),
        })
    }
}

impl Renderer for OpenGLRenderer {
    fn max_gpu_texture_size(&self) -> usize {
        unsafe {
            let mut v: GLint = 0;
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut v as _);
            v as _
        }
    }

    fn upload_atlases(&mut self, atl: AtlasBuilder) -> Result<(), String> {
        assert!(!self.atlases_initialized, "atlases should be initialized only once");

        let (packers, sprites) = atl.into_inner();

        unsafe {
            let textures: Vec<GLuint> = {
                let mut buf = vec![0 as GLuint; packers.len()];
                gl::GenTextures(buf.len() as _, buf.as_mut_ptr());
                for (tex_id, packer) in buf.iter().copied().zip(&packers) {
                    let (width, height) = packer.size();

                    gl::BindTexture(gl::TEXTURE_2D, tex_id);
                    gl::TexImage2D(
                        gl::TEXTURE_2D,    // target
                        0,                 // level
                        gl::RGBA as _,     // internalformat
                        width as _,        // width
                        height as _,       // height
                        0,                 // border ("must be 0")
                        gl::BGRA,          // format
                        gl::UNSIGNED_BYTE, // type
                        ptr::null(),       // data
                    );
                }
                buf
            };

            // upload textures
            let mut current_texture: GLint = 0;
            for (atl_ref, pixels) in &sprites {
                if current_texture != atl_ref.atlas_id as _ {
                    gl::BindTexture(gl::TEXTURE_2D, textures[atl_ref.atlas_id as usize]);
                    current_texture = atl_ref.atlas_id as _;
                }

                gl::TexSubImage2D(
                    gl::TEXTURE_2D,       // target
                    0,                    // level
                    atl_ref.x as _,       // xoffset
                    atl_ref.y as _,       // yoffset
                    atl_ref.w as _,       // width
                    atl_ref.h as _,       // height
                    gl::BGRA,             // format
                    gl::UNSIGNED_BYTE,    // type
                    pixels.as_ptr() as _, // pixels
                );
            }

            // verify it actually worked
            match gl::GetError() {
                0 => (),
                err => return Err(format!("Failed to upload textures to GPU! (OpenGL code {})", err)),
            }

            // store opengl texture handles
            self.texture_ids = textures;
        }

        // store packers, discard pixeldata
        self.atlas_packers = packers;
        self.atlas_refs = sprites.into_iter().map(|(x, _)| x).collect();

        // generate texture handles
        self.atlases_initialized = true;
        Ok(())
    }

    fn draw_sprite(
        &self,
        texture: &Texture,
        x: f64,
        y: f64,
        xscale: f64,
        yscale: f64,
        angle: f64,
        colour: i32,
        alpha: f64,
    ) {
        let atlas_ref = self
            .atlas_refs
            .get(texture.0)
            .expect("Invalid Texture provided to renderer");
        let tex = self
            .texture_ids
            .get(atlas_ref.atlas_id as usize)
            .expect("Invalid Texture provided to renderer (texture_ids)");

        // todo
        println!(
            "Drawing: [atlas ref: {:?}]; [tex: {}]; x: {}, y: {}, xscale: {}, yscale: {}, angle: {}, colour: {}, alpha: {}",
            atlas_ref, tex, x, y, xscale, yscale, angle, colour, alpha
        );
    }

    fn draw(&mut self) {
        unsafe {
            gl::ClearColor(0.2, 0.3, 0.3, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        self.window.swap_buffers();
    }

    fn dump_atlases(&self, path: impl Fn(usize) -> PathBuf) -> io::Result<()> {
        for ((i, texture), packer) in self.texture_ids.iter().enumerate().zip(self.atlas_packers.iter()) {
            let w = BufWriter::new(fs::File::create(&path(i))?);
            let (width, height) = packer.size();
            let mut encoder = png::Encoder::new(w, width as _, height as _);
            encoder.set_color(png::ColorType::RGBA);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let mut buf = vec![0u8; width as usize * height as usize * 4];
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, *texture);
                gl::GetTexImage(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    buf.as_mut_ptr() as *mut _,
                );
            }
            writer.write_image_data(&buf).unwrap();
        }

        Ok(())
    }

    fn should_close(&self) -> bool {
        self.window.should_close()
    }

    fn set_should_close(&mut self, b: bool) {
        self.window.set_should_close(b)
    }

    fn show_window(&mut self) {
        self.window.show()
    }
}

impl Drop for OpenGLRenderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(self.texture_ids.len() as _, self.texture_ids.as_mut_ptr() as *mut _);
        }
    }
}
