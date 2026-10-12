use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use slot_gfx::{Compositor, TexId};
use slot_store::Cart;
use slot_ui::{cart_face_with, CartFace};

use crate::app::{App, FaceWant};
use crate::label_cache::label_art;

/// Label decoding threads: three of the four cores, the shelf runs no game.
const WORKERS: usize = 3;
/// How long a shelf frame waits for faces coming on screen before it draws
/// their plain colour instead: waiting longer is a stutter while scrolling.
pub const FRAME_WAIT: std::time::Duration = std::time::Duration::from_millis(8);

type Key = (usize, usize);

/// A finished face, with the ROM it was drawn for: a rescan can move another
/// cart into its place while it is being drawn.
type Built = (Key, PathBuf, CartFace);

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Key, Cart)>,
    closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct CartFaces {
    queue: Shared,
    built: Receiver<Built>,
    pending: HashSet<Key>,
    asked: Vec<FaceWant>,
    free: Vec<TexId>,
}

impl CartFaces {
    pub fn spawn(root: PathBuf) -> Self {
        let queue: Shared = Arc::default();
        let (outbox, built) = mpsc::channel();
        for n in 0..WORKERS {
            let (queue, outbox, root) = (queue.clone(), outbox.clone(), root.clone());
            let spawned = thread::Builder::new()
                .name(format!("slot-carts-{n}"))
                .spawn(move || work(&queue, &outbox, &root));
            if let Err(e) = spawned {
                eprintln!("slot: cart faces: worker thread failed to start: {e}");
            }
        }
        CartFaces {
            queue,
            built,
            pending: HashSet::new(),
            asked: Vec::new(),
            free: Vec::new(),
        }
    }

    /// Places the faces built so far and waits up to `wait` for the ones on
    /// screen; any still missing show their colour until they arrive.
    pub fn sync(&mut self, app: &mut App, compositor: &mut Compositor, wait: std::time::Duration) {
        self.free.extend(app.shed_faces());
        while let Ok((key, rom, face)) = self.built.try_recv() {
            self.place(app, compositor, key, &rom, face);
        }
        let wants = app.face_wants();
        if wants != self.asked {
            self.requeue(app, &wants);
            self.asked = wants;
        }
        let mut waiting: Vec<Key> = self
            .asked
            .iter()
            .filter(|w| w.on_screen)
            .map(|w| (w.shelf, w.index))
            .collect();
        let began = std::time::Instant::now();
        let missing = waiting.len();
        while !waiting.is_empty() {
            let left = wait.saturating_sub(began.elapsed());
            let Ok((key, rom, face)) = self.built.recv_timeout(left) else {
                break;
            };
            waiting.retain(|k| *k != key);
            self.place(app, compositor, key, &rom, face);
        }
        // A frame that waits here is a stutter while scrolling.
        let waited = began.elapsed();
        if waited > std::time::Duration::from_millis(4) {
            eprintln!(
                "slot: shelf: waited {:.1} ms for {missing} cart faces, {} drawn plain",
                waited.as_secs_f64() * 1e3,
                waiting.len()
            );
        }
    }

    fn requeue(&mut self, app: &App, wants: &[FaceWant]) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        for (key, _) in queue.jobs.drain(..) {
            self.pending.remove(&key);
        }
        for w in wants {
            let key = (w.shelf, w.index);
            if self.pending.contains(&key) {
                continue;
            }
            if let Some(cart) = app.face_cart(w.shelf, w.index) {
                queue.jobs.push_back((key, cart.clone()));
                self.pending.insert(key);
            }
        }
        wake.notify_all();
    }

    fn place(
        &mut self,
        app: &mut App,
        compositor: &mut Compositor,
        key: Key,
        rom: &Path,
        face: CartFace,
    ) {
        self.pending.remove(&key);
        self.asked.retain(|w| (w.shelf, w.index) != key);
        if app.face_cart(key.0, key.1).map(|c| c.rom.as_path()) != Some(rom) {
            return;
        }
        let tex = match self.free.pop() {
            Some(tex) => {
                compositor.update_texture(tex, face.w, face.h, &face.rgba);
                tex
            }
            None => compositor.create_texture(face.w, face.h, &face.rgba),
        };
        self.free.extend(app.set_cart_face(key.0, key.1, tex));
    }
}

impl Drop for CartFaces {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_all();
    }
}

fn work(queue: &Shared, outbox: &Sender<Built>, root: &Path) {
    let (lock, wake) = &**queue;
    loop {
        let (key, cart) = {
            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.closed {
                    return;
                }
                if let Some(job) = q.jobs.pop_front() {
                    break job;
                }
                q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let face = cart_face_with(&cart, label_art(root, &cart));
        if outbox.send((key, cart.rom.clone(), face)).is_err() {
            return;
        }
    }
}
