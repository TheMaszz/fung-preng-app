// src-tauri/src/stream_source.rs
use anyhow::{anyhow, Result};
use std::io::{Read, Seek, SeekFrom};
use ureq::Agent;

pub struct HttpRangeSource {
    url: String,
    total_size: u64,
    pos: u64,
    buffer: Vec<u8>,
    buffer_start: u64,
    chunk_size: u64,
    agent: Agent, // Persistent HTTP connection pool
}

impl HttpRangeSource {
    pub fn new(url: String) -> Result<Self> {
        const INITIAL_CHUNK_SIZE: u64 = 256 * 1024;
        const DEFAULT_CHUNK_SIZE: u64 = 256 * 1024;

        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(10))
            .build();

        let end = INITIAL_CHUNK_SIZE - 1;
        let resp = agent
            .get(&url)
            .set("Range", &format!("bytes=0-{end}"))
            .call()?;

        if resp.status() != 206 {
            return Err(anyhow!(
                "expected 206 Partial Content, got {} — server may not support Range requests",
                resp.status()
            ));
        }

        let total_size = resp
            .header("Content-Range")
            .and_then(|value| value.rsplit('/').next())
            .ok_or_else(|| anyhow!("server didn't report total size in Content-Range"))?
            .parse()?;

        let mut buffer = Vec::new();
        resp.into_reader().read_to_end(&mut buffer)?;

        Ok(Self {
            url,
            total_size,
            pos: 0,
            buffer,
            buffer_start: 0,
            chunk_size: DEFAULT_CHUNK_SIZE,
            agent,
        })
    }

    fn ensure_buffered(&mut self) -> Result<()> {
        let have_data = self.pos >= self.buffer_start
            && self.pos < self.buffer_start + self.buffer.len() as u64;
        if have_data {
            return Ok(());
        }

        let end = (self.pos + self.chunk_size - 1).min(self.total_size.saturating_sub(1));
        let range = format!("bytes={}-{}", self.pos, end);

        // Reuses existing TCP connection
        let resp = self.agent.get(&self.url).set("Range", &range).call()?;

        if resp.status() != 206 {
            return Err(anyhow!(
                "expected 206 Partial Content, got {} — server may not support Range requests",
                resp.status()
            ));
        }

        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf)?;

        self.buffer_start = self.pos;
        self.buffer = buf;
        Ok(())
    }
}

impl Read for HttpRangeSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.total_size {
            return Ok(0);
        }
        self.ensure_buffered()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let offset_in_buffer = (self.pos - self.buffer_start) as usize;
        let available = self.buffer.len() - offset_in_buffer;
        let to_copy = available.min(buf.len());

        buf[..to_copy].copy_from_slice(&self.buffer[offset_in_buffer..offset_in_buffer + to_copy]);
        self.pos += to_copy as u64;
        Ok(to_copy)
    }
}

impl Seek for HttpRangeSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(offset) => self.total_size as i64 + offset,
            SeekFrom::Current(offset) => self.pos as i64 + offset,
        };
        if new_pos < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek to negative position",
            ));
        }
        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for HttpRangeSource {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.total_size)
    }
}
