use std::path::{Path, PathBuf};

use rocket::fs::NamedFile;
use rocket::http::{Method, Status};
use rocket::outcome::IntoOutcome;
use rocket::response::Responder;
use rocket::route::{Handler, Outcome, Route};
use rocket::{Data, Request, figment};

#[derive(Debug, Clone)]
pub struct SingleFileServer {
    file: PathBuf,
    rank: isize,
}

impl SingleFileServer {
    const DEFAULT_RANK: isize = 10;

    #[track_caller]
    pub fn new<P: AsRef<Path>>(file: P) -> Self {
        use rocket::yansi::Paint;

        let file = file.as_ref();
        if !file.exists() {
            let file = file.display();
            error!("SingleFileServer path '{}' is not a file.", file.primary());
            warn_!("Aborting early to prevent inevitable handler error.");
            panic!("invalid file: refusing to continue");
        }

        SingleFileServer {
            file: file.into(),
            rank: Self::DEFAULT_RANK,
        }
    }

    pub fn rank(mut self, rank: isize) -> Self {
        self.rank = rank;
        self
    }
}

impl From<SingleFileServer> for Vec<Route> {
    fn from(server: SingleFileServer) -> Self {
        let source = figment::Source::File(server.file.clone());
        let mut route = Route::ranked(server.rank, Method::Get, "/<path..>", server);
        route.name = Some(format!("SingleFileServer: {}", source).into());
        vec![route]
    }
}

#[rocket::async_trait]
impl Handler for SingleFileServer {
    async fn handle<'r>(&self, req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r> {
        let file = NamedFile::open(&self.file).await;
        file.respond_to(req).or_forward((data, Status::NotFound))
    }
}
