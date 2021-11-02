use xtm_rust::AsyncDispatcher;

use tonic::{transport::Server, Request, Response, Status};

use userapi::user_api_server::{UserApi, UserApiServer};
use userapi::{CreateUserReply, CreateUserRequest};

use uuid::Uuid;

use tarantool::space::Space;

pub mod userapi {
    tonic::include_proto!("userapi");
}

pub struct UserAPIService {
    dispatcher: AsyncDispatcher<&'static mlua::Lua>,
}

#[tonic::async_trait]
impl UserApi for UserAPIService {
    async fn create_user(
        &self,
        request: Request<CreateUserRequest>,
    ) -> Result<Response<CreateUserReply>, Status> {
        println!("Got a request from {:?}", request.remote_addr());

        let msg = request.into_inner();
        let username = msg.username.clone();

        let (uuid, username) = self.dispatcher.call(move |_| {
            let mut space = Space::find("users").unwrap();

            let tuple = (Uuid::new_v4(), username);
            space.replace(&tuple).unwrap();

            tuple
        }).await.unwrap();

        Ok(Response::new(userapi::CreateUserReply {
            uuid: uuid.to_string(),
            username,
        }))
    }
}

pub (crate) async fn module_main(dispatcher: AsyncDispatcher<&'static mlua::Lua>) {
    let addr = "0.0.0.0:50051".parse().unwrap();
    let service = UserAPIService { dispatcher };

    println!("gRPC listening on {}", addr);

    Server::builder()
        .add_service(UserApiServer::new(service))
        .serve(addr)
        .await
        .unwrap();
}
