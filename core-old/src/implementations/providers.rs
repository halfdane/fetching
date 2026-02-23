//! Provider implementations that wrap librespot types.

use async_trait::async_trait;
use librespot_core::file_id::FileId;
use librespot_metadata::audio::AudioFileFormat;

/// Real implementation for librespot_metadata::track::Track
#[derive(Debug)]
pub struct OwnedLibrespotTrackProvider {
    pub track: librespot_metadata::track::Track,
}

#[async_trait]
impl crate::traits::TrackMetadataProvider for OwnedLibrespotTrackProvider {
    async fn name(&self) -> String {
        self.track.name.clone()
    }
    async fn album_id(&self) -> String {
        self.track.album.id.to_string()
    }
    async fn album_name(&self) -> String {
        self.track.album.name.clone()
    }
    async fn artist_names(&self) -> Vec<String> {
        self.track.artists.iter().map(|a| a.name.clone()).collect()
    }
    async fn duration_ms(&self) -> u32 {
        self.track.duration as u32
    }
    async fn date(&self) -> Option<String> {
        let date_obj = self.track.album.date;
        let year = date_obj.year();
        let month = date_obj.month() as u8;
        let day = date_obj.day();

        if year > 0 && month > 0 && day > 0 {
            Some(format!("{:04}-{:02}-{:02}", year, month, day))
        } else if year > 0 {
            Some(year.to_string())
        } else {
            None
        }
    }
    async fn track_number(&self) -> u32 {
        self.track.number as u32
    }
    async fn get_file_id(&self, format: &AudioFileFormat) -> Option<FileId> {
        self.track.files.get(format).copied()
    }

    async fn album_artist_names(&self) -> Vec<String> {
        self.track
            .album
            .artists
            .iter()
            .map(|a| a.name.clone())
            .collect()
    }
    async fn disc_number(&self) -> u32 {
        self.track.disc_number as u32
    }
    async fn genres(&self) -> Vec<String> {
        self.track.tags.clone()
    }
    async fn isrc(&self) -> Option<String> {
        self.track
            .external_ids
            .iter()
            .find(|eid| eid.external_type == "isrc")
            .map(|eid| eid.id.clone())
    }
    async fn label(&self) -> Option<String> {
        if !self.track.album.label.is_empty() {
            Some(self.track.album.label.clone())
        } else {
            None
        }
    }

    async fn get_album_cover_file_id(&self, index: usize) -> Option<FileId> {
        self.track.album.covers.get(index).map(|cover| cover.id)
    }

    async fn alternative_uris(&self) -> Vec<String> {
        self.track
            .alternatives
            .iter()
            .map(|uri| uri.to_string())
            .collect()
    }
}
