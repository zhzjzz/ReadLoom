use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
};

use readloom_core::{BlockId, InsertSide, ReadloomCore, ValidatedImageAsset};

const MAXIMUM_PENDING_IMPORTS: usize = 2;

pub(crate) struct EpubImageImportRequest {
    pub(crate) request_id: u64,
    pub(crate) document_path: PathBuf,
    pub(crate) revision: u64,
    pub(crate) anchor_block_id: BlockId,
    pub(crate) side: InsertSide,
    pub(crate) alt_text: String,
    pub(crate) source_path: PathBuf,
}

pub(crate) struct EpubImageImportResult {
    pub(crate) request_id: u64,
    pub(crate) document_path: PathBuf,
    pub(crate) revision: u64,
    pub(crate) anchor_block_id: BlockId,
    pub(crate) side: InsertSide,
    pub(crate) alt_text: String,
    pub(crate) result: Result<ValidatedImageAsset, String>,
}

pub(crate) fn spawn_epub_image_import_worker(
    core: Arc<ReadloomCore>,
) -> (
    mpsc::SyncSender<EpubImageImportRequest>,
    mpsc::Receiver<EpubImageImportResult>,
) {
    let (request_sender, request_receiver) =
        mpsc::sync_channel::<EpubImageImportRequest>(MAXIMUM_PENDING_IMPORTS);
    let (result_sender, result_receiver) = mpsc::channel::<EpubImageImportResult>();
    thread::spawn(move || {
        while let Ok(request) = request_receiver.recv() {
            let result = core
                .validate_epub_image(&request.source_path)
                .map_err(|error| error.to_string());
            if result_sender
                .send(EpubImageImportResult {
                    request_id: request.request_id,
                    document_path: request.document_path,
                    revision: request.revision,
                    anchor_block_id: request.anchor_block_id,
                    side: request.side,
                    alt_text: request.alt_text,
                    result,
                })
                .is_err()
            {
                return;
            }
        }
    });
    (request_sender, result_receiver)
}
