//! Decoder threads and a frame cache.
//!
//! Each file is pinned to one worker so its decoder keeps its position for
//! forward playback. Requests carry a `slot` (the viewer, one thumbnail, …):
//! when a worker falls behind, only the newest request per slot is decoded,
//! so scrubbing never queues up stale frames.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;

use crate::{MediaError, Picture, VideoDecoder};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    path: PathBuf,
    /// Milliseconds, so near-identical times share an entry.
    ms: i64,
    max_w: u32,
    max_h: u32,
}

struct Cache {
    map: HashMap<CacheKey, Arc<Picture>>,
    order: VecDeque<CacheKey>,
    bytes: usize,
    limit: usize,
}

impl Cache {
    fn get(&mut self, key: &CacheKey) -> Option<Arc<Picture>> {
        let hit = self.map.get(key).cloned()?;
        if let Some(i) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(i).expect("index from position");
            self.order.push_back(k);
        }
        Some(hit)
    }

    fn put(&mut self, key: CacheKey, picture: Arc<Picture>) {
        if self.map.contains_key(&key) {
            return;
        }
        self.bytes += picture.bgra.len();
        self.order.push_back(key.clone());
        self.map.insert(key, picture);
        while self.bytes > self.limit {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(p) = self.map.remove(&old) {
                self.bytes -= p.bgra.len();
            }
        }
    }
}

pub struct FrameRequest {
    pub path: PathBuf,
    /// Seconds into the file.
    pub time: f64,
    pub max_w: u32,
    pub max_h: u32,
    /// Requests sharing a slot replace each other while queued.
    pub slot: u64,
}

struct Job {
    req: FrameRequest,
    key: CacheKey,
    reply: oneshot::Sender<Result<Arc<Picture>, MediaError>>,
}

pub struct FramePool {
    workers: Vec<Sender<Job>>,
    cache: Arc<Mutex<Cache>>,
}

impl FramePool {
    /// `workers` threads; `cache_mb` megabytes of decoded frames.
    pub fn new(workers: usize, cache_mb: usize) -> Self {
        crate::init();
        let cache = Arc::new(Mutex::new(Cache {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limit: cache_mb << 20,
        }));
        let workers = (0..workers.max(1))
            .map(|i| {
                let (tx, rx) = channel::<Job>();
                let cache = cache.clone();
                std::thread::Builder::new()
                    .name(format!("videode-decode-{i}"))
                    .spawn(move || worker(rx, cache))
                    .expect("spawn decoder thread");
                tx
            })
            .collect();
        Self { workers, cache }
    }

    fn key(req: &FrameRequest) -> CacheKey {
        CacheKey {
            path: req.path.clone(),
            ms: (req.time * 1000.).round() as i64,
            max_w: req.max_w,
            max_h: req.max_h,
        }
    }

    /// A cached frame, without decoding.
    pub fn cached(&self, req: &FrameRequest) -> Option<Arc<Picture>> {
        self.cache.lock().ok()?.get(&Self::key(req))
    }

    /// Decodes a frame in the background. Resolves to an error if the
    /// request was superseded by a newer one in the same slot.
    pub fn request(
        &self,
        req: FrameRequest,
    ) -> oneshot::Receiver<Result<Arc<Picture>, MediaError>> {
        let (reply, rx) = oneshot::channel();
        let key = Self::key(&req);
        if let Some(hit) = self.cache.lock().ok().and_then(|mut c| c.get(&key)) {
            reply.send(Ok(hit)).ok();
            return rx;
        }
        let mut h = DefaultHasher::new();
        req.path.hash(&mut h);
        let worker = &self.workers[h.finish() as usize % self.workers.len()];
        worker.send(Job { req, key, reply }).ok();
        rx
    }
}

fn worker(rx: Receiver<Job>, cache: Arc<Mutex<Cache>>) {
    let mut decoders: HashMap<PathBuf, VideoDecoder> = HashMap::new();
    while let Ok(first) = rx.recv() {
        // Take everything queued and keep only the newest job per slot.
        let mut jobs = vec![first];
        jobs.extend(rx.try_iter());
        let mut newest: Vec<Job> = Vec::with_capacity(jobs.len());
        for job in jobs {
            if let Some(i) = newest.iter().position(|j| j.req.slot == job.req.slot) {
                let old = std::mem::replace(&mut newest[i], job);
                old.reply.send(Err(MediaError::Superseded)).ok();
            } else {
                newest.push(job);
            }
        }
        for job in newest {
            if job.reply.is_canceled() {
                continue;
            }
            let result = decode(
                &mut decoders,
                &job.req.path,
                job.req.time,
                job.req.max_w,
                job.req.max_h,
            );
            if decoders.len() > 8 {
                // Keep the most recent few files open.
                let keep = job.req.path.clone();
                decoders.retain(|p, _| *p == keep);
            }
            let result = result.map(Arc::new);
            if let (Ok(p), Ok(mut c)) = (&result, cache.lock()) {
                c.put(job.key.clone(), p.clone());
            }
            job.reply.send(result).ok();
        }
    }
}

fn decode(
    decoders: &mut HashMap<PathBuf, VideoDecoder>,
    path: &Path,
    time: f64,
    max_w: u32,
    max_h: u32,
) -> Result<Picture, MediaError> {
    if !decoders.contains_key(path) {
        decoders.insert(path.to_path_buf(), VideoDecoder::open(path)?);
    }
    let d = decoders.get_mut(path).expect("inserted above");
    let result = d.frame_at(time, max_w, max_h);
    if result.is_err() {
        // A broken decoder is reopened next time.
        decoders.remove(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_oldest_beyond_limit() {
        let mut c = Cache {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limit: 250,
        };
        let pic = |n: u8| {
            Arc::new(Picture {
                width: 1,
                height: 1,
                bgra: vec![n; 100],
                time: 0.,
            })
        };
        let key = |ms| CacheKey {
            path: "a".into(),
            ms,
            max_w: 1,
            max_h: 1,
        };
        c.put(key(1), pic(1));
        c.put(key(2), pic(2));
        assert!(c.get(&key(1)).is_some(), "touch 1 so 2 is oldest");
        c.put(key(3), pic(3));
        assert!(c.get(&key(2)).is_none());
        assert!(c.get(&key(1)).is_some() && c.get(&key(3)).is_some());
        assert!(c.bytes <= 250);
    }
}
