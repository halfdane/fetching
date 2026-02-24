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
