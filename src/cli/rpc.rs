use alloy_rpc_types_engine::JwtSecret;
use clap::Parser;
use eyre::{Result, eyre};
use hyper::Uri;
use paste::paste;
use std::path::PathBuf;

use crate::client::{fanout::FanoutWrite, http::HttpClient};

macro_rules! define_rpc_args {
    ($(($name:ident, $prefix:ident)),*) => {
        $(
            paste! {
                #[derive(Parser, Debug, Clone, PartialEq, Eq)]
                pub struct $name {
                    /// RPC Server 0
                    #[arg(long, env)]
                    pub [<$prefix _url_0>]: Uri,

                    /// RPC Server 1
                    #[arg(long, env)]
                    pub [<$prefix _url_1>]: Uri,

                    /// RPC Server 2
                    #[arg(long, env)]
                    pub [<$prefix _url_2>]: Uri,

                    /// Hex encoded JWT secret to use for an authenticated RPC server.
                    #[arg(long, env, value_name = "HEX")]
                    pub [<$prefix _jwt_token>]: Option<JwtSecret>,

                    /// Path to a JWT secret to use for an authenticated RPC server.
                    #[arg(long, env, value_name = "PATH")]
                    pub [<$prefix _jwt_path>]: Option<PathBuf>,

                    /// Timeout for http calls in milliseconds
                    #[arg(long, env, default_value_t = 1000)]
                    pub [<$prefix _timeout>]: u64,
                }

                impl $name {
                    fn get_jwt(&self) -> Result<JwtSecret> {
                        if let Some(secret) = &self.[<$prefix _jwt_token>] {
                            Ok(secret.clone())
                        } else if let Some(path) = &self.[<$prefix _jwt_path>] {
                            Ok(JwtSecret::from_file(path)?)
                        } else {
                            Err(eyre!(
                                "No JWT secret provided. Please provide either a hex encoded JWT secret or a path to a file containing the JWT secret."
                            ))
                        }
                    }

                    pub fn build(&self) -> Result<FanoutWrite> {
                        let jwt = self.get_jwt()?;
                        let client_0 = HttpClient::new(self.[<$prefix _url_0>].clone(), jwt.clone());
                        let client_1 = HttpClient::new(self.[<$prefix _url_1>].clone(), jwt.clone());
                        let client_2 = HttpClient::new(self.[<$prefix _url_2>].clone(), jwt);
                        Ok(FanoutWrite::new(vec![client_0, client_1, client_2]))
                    }
                }
            }
        )*
    };
}

define_rpc_args!((BuilderTargets, builder), (L2Targets, l2));
