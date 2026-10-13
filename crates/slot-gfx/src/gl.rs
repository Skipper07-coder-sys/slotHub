use crate::surface::{GfxError, Surface};
use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static ES: AtomicBool = AtomicBool::new(false);

type GetProgramBinary = unsafe extern "C" fn(
    gl::types::GLuint,
    gl::types::GLsizei,
    *mut gl::types::GLsizei,
    *mut gl::types::GLenum,
    *mut c_void,
);
type ProgramBinary =
    unsafe extern "C" fn(gl::types::GLuint, gl::types::GLenum, *const c_void, gl::types::GLsizei);

/// The driver's program binary calls, when it has them: a linked program
/// saved once loads on the next start without compiling its shaders again.
struct Binaries {
    get: GetProgramBinary,
    put: ProgramBinary,
    /// Names the driver, so a binary from another driver is never offered.
    driver: String,
}

static BINARIES: OnceLock<Option<Binaries>> = OnceLock::new();
static CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Where linked programs are kept between starts. Without one, every start
/// compiles its shaders.
pub fn set_program_cache(dir: PathBuf) {
    *CACHE_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
}

fn gl_string(name: gl::types::GLenum) -> String {
    let s = unsafe { gl::GetString(name) };
    if s.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) }
        .to_string_lossy()
        .into_owned()
}

fn load_binaries(surface: &dyn Surface) -> Option<Binaries> {
    const NUM_PROGRAM_BINARY_FORMATS: gl::types::GLenum = 0x87FE;
    let mut formats = 0;
    unsafe { gl::GetIntegerv(NUM_PROGRAM_BINARY_FORMATS, &mut formats) };
    unsafe { gl::GetError() };
    if formats <= 0 {
        return None;
    }
    let find = |names: [&str; 2]| {
        names
            .iter()
            .map(|n| surface.proc_address(n))
            .find(|p| !p.is_null())
    };
    let get = find(["glGetProgramBinaryOES", "glGetProgramBinary"])?;
    let put = find(["glProgramBinaryOES", "glProgramBinary"])?;
    Some(Binaries {
        get: unsafe { std::mem::transmute::<*const c_void, GetProgramBinary>(get) },
        put: unsafe { std::mem::transmute::<*const c_void, ProgramBinary>(put) },
        driver: format!(
            "{}|{}|{}",
            gl_string(gl::VENDOR),
            gl_string(gl::RENDERER),
            gl_string(gl::VERSION)
        ),
    })
}

pub fn load(surface: &dyn Surface) {
    gl::load_with(|name| surface.proc_address(name));
    let _ = BINARIES.set(load_binaries(surface));
    let version = unsafe { gl::GetString(gl::VERSION) };
    let es = !version.is_null()
        && unsafe { std::ffi::CStr::from_ptr(version as *const std::ffi::c_char) }
            .to_string_lossy()
            .starts_with("OpenGL ES");
    ES.store(es, Ordering::Relaxed);
}

pub fn es() -> bool {
    ES.load(Ordering::Relaxed)
}

const LUMINANCE: gl::types::GLenum = 0x1909;

pub fn internal_format(format: gl::types::GLenum) -> gl::types::GLint {
    match (es(), format) {
        (true, _) => format as gl::types::GLint,
        (false, gl::RED) => gl::R8 as gl::types::GLint,
        (false, _) => gl::RGBA8 as gl::types::GLint,
    }
}

fn bytes_per_pixel(format: gl::types::GLenum) -> u32 {
    match format {
        gl::RED | LUMINANCE => 1,
        _ => 4,
    }
}

fn shader(kind: gl::types::GLenum, src: &str) -> Result<gl::types::GLuint, GfxError> {
    let Ok(csrc) = CString::new(src) else {
        return Err(GfxError::Shader("source contains a nul byte".into()));
    };
    unsafe {
        let id = gl::CreateShader(kind);
        gl::ShaderSource(id, 1, &csrc.as_ptr(), std::ptr::null());
        gl::CompileShader(id);
        let mut ok = 0;
        gl::GetShaderiv(id, gl::COMPILE_STATUS, &mut ok);
        if ok == 0 {
            let log = info_log(id, gl::GetShaderiv, gl::GetShaderInfoLog);
            gl::DeleteShader(id);
            return Err(GfxError::Shader(log));
        }
        Ok(id)
    }
}

/// FNV-1a over the driver and both sources: a changed shader or driver
/// misses the cache.
fn program_key(driver: &str, vert: &str, frag: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in [driver, "\0", vert, "\0", frag] {
        for b in part.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

fn cached_path(vert: &str, frag: &str) -> Option<(&'static Binaries, PathBuf)> {
    let bins = BINARIES.get()?.as_ref()?;
    let dir = CACHE_DIR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()?;
    let key = program_key(&bins.driver, vert, frag);
    Some((bins, dir.join(format!("{key:016x}.bin"))))
}

/// A program from the cache: four bytes of the driver's format, then the
/// binary. None when it is missing or the driver turns it down.
fn cached_program(bins: &Binaries, path: &std::path::Path) -> Option<gl::types::GLuint> {
    let data = std::fs::read(path).ok()?;
    if data.len() <= 4 {
        return None;
    }
    let format = u32::from_le_bytes(data[..4].try_into().ok()?);
    let body = &data[4..];
    unsafe {
        let p = gl::CreateProgram();
        (bins.put)(
            p,
            format,
            body.as_ptr() as *const c_void,
            body.len() as gl::types::GLsizei,
        );
        let mut ok = 0;
        gl::GetProgramiv(p, gl::LINK_STATUS, &mut ok);
        if ok == 0 {
            gl::GetError();
            gl::DeleteProgram(p);
            let _ = std::fs::remove_file(path);
            return None;
        }
        Some(p)
    }
}

fn save_program(bins: &Binaries, path: &std::path::Path, p: gl::types::GLuint) {
    const PROGRAM_BINARY_LENGTH: gl::types::GLenum = 0x8741;
    let mut len = 0;
    unsafe { gl::GetProgramiv(p, PROGRAM_BINARY_LENGTH, &mut len) };
    if len <= 0 {
        unsafe { gl::GetError() };
        return;
    }
    let mut data = vec![0u8; 4 + len as usize];
    let (mut written, mut format) = (0, 0);
    unsafe {
        (bins.get)(
            p,
            len,
            &mut written,
            &mut format,
            data[4..].as_mut_ptr() as *mut c_void,
        );
    }
    if written <= 0 {
        unsafe { gl::GetError() };
        return;
    }
    data.truncate(4 + written as usize);
    data[..4].copy_from_slice(&format.to_le_bytes());
    let tmp = path.with_extension("tmp");
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&tmp, &data))
        .and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = saved {
        eprintln!("slot: gfx: could not keep a compiled program: {e}");
        let _ = std::fs::remove_file(&tmp);
    }
}

pub fn program(vert: &str, frag: &str) -> Result<gl::types::GLuint, GfxError> {
    let cache = cached_path(vert, frag);
    if let Some((bins, path)) = &cache {
        if let Some(p) = cached_program(bins, path) {
            return Ok(p);
        }
    }
    let p = compile_program(vert, frag)?;
    if let Some((bins, path)) = &cache {
        save_program(bins, path, p);
    }
    Ok(p)
}

fn compile_program(vert: &str, frag: &str) -> Result<gl::types::GLuint, GfxError> {
    let vs = shader(gl::VERTEX_SHADER, vert)?;
    let fs = match shader(gl::FRAGMENT_SHADER, frag) {
        Ok(fs) => fs,
        Err(e) => {
            unsafe { gl::DeleteShader(vs) };
            return Err(e);
        }
    };
    unsafe {
        let p = gl::CreateProgram();
        gl::AttachShader(p, vs);
        gl::AttachShader(p, fs);
        if !es() && BINARIES.get().is_some_and(Option::is_some) {
            const RETRIEVABLE_HINT: gl::types::GLenum = 0x8257;
            gl::ProgramParameteri(p, RETRIEVABLE_HINT, gl::TRUE as gl::types::GLint);
        }
        if let Ok(pos) = CString::new("a_pos") {
            gl::BindAttribLocation(p, crate::quad::POS_LOCATION, pos.as_ptr());
        }
        gl::LinkProgram(p);
        gl::DeleteShader(vs);
        gl::DeleteShader(fs);
        let mut ok = 0;
        gl::GetProgramiv(p, gl::LINK_STATUS, &mut ok);
        if ok == 0 {
            let log = info_log(p, gl::GetProgramiv, gl::GetProgramInfoLog);
            gl::DeleteProgram(p);
            return Err(GfxError::Shader(log));
        }
        Ok(p)
    }
}

unsafe fn info_log(
    id: gl::types::GLuint,
    get_iv: unsafe fn(gl::types::GLuint, gl::types::GLenum, *mut gl::types::GLint),
    get_log: unsafe fn(
        gl::types::GLuint,
        gl::types::GLsizei,
        *mut gl::types::GLsizei,
        *mut gl::types::GLchar,
    ),
) -> String {
    let mut len = 0;
    get_iv(id, gl::INFO_LOG_LENGTH, &mut len);
    if len <= 0 {
        return "no info log".into();
    }
    let mut buf = vec![0u8; len as usize];
    get_log(
        id,
        len,
        std::ptr::null_mut(),
        buf.as_mut_ptr() as *mut gl::types::GLchar,
    );
    while buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

pub fn texture(
    w: u32,
    h: u32,
    filter: gl::types::GLenum,
    wrap: gl::types::GLenum,
    format: gl::types::GLenum,
    data: Option<&[u8]>,
) -> gl::types::GLuint {
    let pixels = match data {
        Some(d) if d.len() >= (w * h * bytes_per_pixel(format)) as usize => {
            d.as_ptr() as *const std::ffi::c_void
        }
        _ => std::ptr::null(),
    };
    unsafe {
        let mut tex = 0;
        gl::GenTextures(1, &mut tex);
        gl::BindTexture(gl::TEXTURE_2D, tex);
        gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            internal_format(format),
            w as i32,
            h as i32,
            0,
            format,
            gl::UNSIGNED_BYTE,
            pixels,
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, filter as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, filter as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, wrap as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, wrap as i32);
        tex
    }
}

pub fn uniform_location(program: gl::types::GLuint, name: &str) -> gl::types::GLint {
    match CString::new(name) {
        Ok(c) => unsafe { gl::GetUniformLocation(program, c.as_ptr()) },
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::program_key;

    #[test]
    fn a_changed_shader_or_driver_misses_the_cache() {
        let k = program_key("Mali-G31", "v", "f");
        assert_eq!(k, program_key("Mali-G31", "v", "f"));
        assert_ne!(k, program_key("Mali-G31", "v", "g"));
        assert_ne!(k, program_key("Mali-G57", "v", "f"));
        assert_ne!(program_key("d", "vf", ""), program_key("d", "v", "f"));
    }
}
