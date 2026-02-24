use crate::spotify_api::SpotifyCover;
use async_trait::async_trait;
use librespot_core::{FileId, Session};

#[derive(Clone)]
pub struct LibrespotCoverFetcher {
    session: Session,
}

impl LibrespotCoverFetcher {
    pub async fn new(session: &Session) -> anyhow::Result<Self> {
        Ok(Self {
            session: session.clone(),
        })
    }
}

#[async_trait]
impl SpotifyCover for LibrespotCoverFetcher {
    async fn fetch_cover(&self, cover_id: &str) -> anyhow::Result<Vec<u8>> {
        let bytes = hex::decode(&cover_id).expect("valid hex");
        let file_id = FileId::from_raw(&bytes);

        let image_bytes = futures::executor::block_on(self.session.spclient().get_image(&file_id))?;
        Ok(image_bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use librespot_core::FileId;

    /// Documents how to round-trip a librespot FileId through its hex string representation.
    /// FileId::to_string() produces lowercase hex; FileId::from_raw() reconstructs from raw bytes.
    /// This is the same pattern used in fetch_cover when converting a cover_id string → FileId.
    #[test]
    fn should_round_trip_file_id_through_hex_string() {
        // given
        let bytes: [u8; 20] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
            0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14,
        ];
        let original = FileId(bytes);

        // when
        let hex_str = original.to_string();
        let reconstructed = FileId::from_raw(&hex::decode(&hex_str).expect("valid hex"));

        // then
        assert_eq!(original, reconstructed);
    }
}
