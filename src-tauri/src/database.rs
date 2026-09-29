use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AppDatabase {
    connection: Mutex<Connection>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct ListenHistoryEntry {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub count: u32,
    #[serde(rename = "lastPlayedAt")]
    pub last_played_at: i64,
}

#[derive(Debug, Serialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct PlaylistTrack {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration_secs: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub position: i64,
}

impl AppDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS listen_history (
                     video_id TEXT PRIMARY KEY NOT NULL,
                     title TEXT NOT NULL,
                     channel TEXT NOT NULL,
                     play_count INTEGER NOT NULL DEFAULT 0,
                     last_played_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS playlists (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     name TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS playlist_tracks (
                     playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                     video_id TEXT NOT NULL,
                     title TEXT NOT NULL,
                     channel TEXT NOT NULL,
                     duration_secs REAL,
                     thumbnail_url TEXT,
                     position INTEGER NOT NULL,
                     PRIMARY KEY (playlist_id, video_id)
                 );",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn now_millis() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    pub fn record_listen(&self, id: &str, title: &str, channel: &str) -> Result<(), String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        connection
            .execute(
                "INSERT INTO listen_history (video_id, title, channel, play_count, last_played_at)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT(video_id) DO UPDATE SET
                     title = excluded.title,
                     channel = excluded.channel,
                     play_count = listen_history.play_count + 1,
                     last_played_at = excluded.last_played_at",
                params![id, title, channel, Self::now_millis()],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn import_legacy_history(&self, entries: Vec<ListenHistoryEntry>) -> Result<(), String> {
        let mut connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        for entry in entries {
            transaction
                .execute(
                    "INSERT INTO listen_history (video_id, title, channel, play_count, last_played_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(video_id) DO UPDATE SET
                         title = excluded.title,
                         channel = excluded.channel,
                         play_count = MAX(listen_history.play_count, excluded.play_count),
                         last_played_at = MAX(listen_history.last_played_at, excluded.last_played_at)",
                    params![entry.id, entry.title, entry.channel, entry.count, entry.last_played_at],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn listen_history(&self) -> Result<Vec<ListenHistoryEntry>, String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        let mut statement = connection
            .prepare(
                "SELECT video_id, title, channel, play_count, last_played_at
                 FROM listen_history ORDER BY play_count DESC, last_played_at DESC LIMIT 30",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(ListenHistoryEntry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    channel: row.get(2)?,
                    count: row.get(3)?,
                    last_played_at: row.get(4)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn clear_listen_history(&self) -> Result<(), String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        connection
            .execute("DELETE FROM listen_history", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>, String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        let mut statement = connection
            .prepare("SELECT id, name FROM playlists ORDER BY created_at, id")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn create_playlist(&self, name: &str) -> Result<Playlist, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Playlist name cannot be empty".to_string());
        }
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        connection
            .execute(
                "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
                params![name, Self::now_millis()],
            )
            .map_err(|error| error.to_string())?;
        Ok(Playlist {
            id: connection.last_insert_rowid(),
            name: name.to_string(),
        })
    }

    pub fn delete_playlist(&self, playlist_id: i64) -> Result<(), String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        connection
            .execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn add_playlist_track(
        &self,
        playlist_id: i64,
        track: PlaylistTrack,
    ) -> Result<bool, String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        let position: i64 = connection
            .query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?1",
                [playlist_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let inserted = connection
            .execute(
                "INSERT INTO playlist_tracks
                 (playlist_id, video_id, title, channel, duration_secs, thumbnail_url, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(playlist_id, video_id) DO NOTHING",
                params![
                    playlist_id,
                    track.id,
                    track.title,
                    track.channel,
                    track.duration_secs,
                    track.thumbnail_url,
                    position
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(inserted > 0)
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<PlaylistTrack>, String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        let mut statement = connection
            .prepare(
                "SELECT video_id, title, channel, duration_secs, thumbnail_url, position
                 FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([playlist_id], |row| {
                Ok(PlaylistTrack {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    channel: row.get(2)?,
                    duration_secs: row.get(3)?,
                    thumbnail_url: row.get(4)?,
                    position: row.get(5)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn reorder_playlist_tracks(
        &self,
        playlist_id: i64,
        track_ids: &[String],
    ) -> Result<(), String> {
        let mut connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        for (position, track_id) in track_ids.iter().enumerate() {
            transaction
                .execute(
                    "UPDATE playlist_tracks SET position = ?1
                     WHERE playlist_id = ?2 AND video_id = ?3",
                    params![position as i64, playlist_id, track_id],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn remove_playlist_track(&self, playlist_id: i64, track_id: &str) -> Result<(), String> {
        let connection = self.connection.lock().map_err(|_| "Database lock poisoned")?;
        connection
            .execute(
                "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND video_id = ?2",
                params![playlist_id, track_id],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AppDatabase, PlaylistTrack};

    #[test]
    fn persists_history_and_playlist_tracks() {
        let path = std::env::temp_dir().join(format!(
            "fung-pleng-test-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let database = AppDatabase::open(&path).unwrap();
        database.record_listen("track-1", "Song", "Artist").unwrap();
        database.record_listen("track-1", "Song", "Artist").unwrap();

        let playlist = database.create_playlist("Favorites").unwrap();
        assert!(database
            .add_playlist_track(
                playlist.id,
                PlaylistTrack {
                    id: "track-1".to_string(),
                    title: "Song".to_string(),
                    channel: "Artist".to_string(),
                    duration_secs: Some(180.0),
                    thumbnail_url: None,
                    position: 0,
                },
            )
            .unwrap());
        drop(database);

        let database = AppDatabase::open(&path).unwrap();
        let history = database.listen_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].count, 2);
        let tracks = database.playlist_tracks(playlist.id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "track-1");

        database.clear_listen_history().unwrap();
        assert!(database.listen_history().unwrap().is_empty());
        drop(database);
        std::fs::remove_file(path).unwrap();
    }
}