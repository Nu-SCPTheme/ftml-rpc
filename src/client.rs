/*
 * client.rs
 *
 * ftml-rpc - RPC server to convert Wikidot code to HTML
 * Copyright (C) 2019-2020 Ammon Smith
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

use crate::api::{FtmlClient, PROTOCOL_VERSION};
use crate::Result;
use ftml::html::HtmlOutput;
use ftml::PageInfoOwned;
use serde_json::Value;
use std::net::SocketAddr;
use std::time::Duration;
use std::{io, mem};
use tarpc::rpc::client::Config as RpcConfig;
use tarpc::rpc::context;
use tarpc::serde_transport::tcp;
use tokio::time::timeout;
use tokio_serde::formats::Json;

macro_rules! ctx {
    () => {
        context::current()
    };
}

macro_rules! retry {
    ($self:expr, $new_future:expr,) => {
        retry!($self, $new_future);
    };

    ($self:expr, $new_future:expr) => {{
        use io::{Error, ErrorKind};

        let mut result = None;

        for _ in 0..5 {
            let fut = $new_future;

            match timeout($self.timeout, fut).await {
                Ok(resp) => {
                    result = Some(resp?);
                    break;
                }
                Err(_) => {
                    warn!(
                        "Remote call timed out ({:.3} seconds)",
                        $self.timeout.as_secs_f64(),
                    );

                    // Attempt to reconnect
                    if let Err(error) = $self.reconnect().await {
                        warn!("Failed to reconnect to remote server");

                        return Err(error);
                    }
                }
            }
        }

        result
            .ok_or_else(|| Error::new(ErrorKind::TimedOut, "Remote server not responding in time"))
    }};
}

#[derive(Debug)]
pub struct Client {
    client: FtmlClient,
    address: SocketAddr,
    timeout: Duration,
}

impl Client {
    pub async fn new(address: SocketAddr, timeout: Duration) -> io::Result<Self> {
        let transport = tcp::connect(&address, Json::default()).await?;
        let config = RpcConfig::default();
        let client = FtmlClient::new(config, transport).spawn()?;

        Ok(Client {
            client,
            address,
            timeout,
        })
    }

    async fn reconnect(&mut self) -> io::Result<()> {
        debug!("Attempting to reconnect to source...");
        let mut client = Self::new(self.address, self.timeout).await?;

        debug!("Successfully reconnected");
        mem::swap(self, &mut client);

        Ok(())
    }

    // Misc
    pub async fn protocol(&mut self) -> io::Result<String> {
        info!("Method: protocol");

        let version = retry!(self, self.client.protocol(context::current()))?;

        if PROTOCOL_VERSION != version {
            warn!(
                "Protocol version mismatch! Client: {}, server: {}",
                PROTOCOL_VERSION, version,
            );
        }

        Ok(version)
    }

    pub async fn ping(&mut self) -> io::Result<()> {
        info!("Method: ping");

        retry!(self, self.client.ping(ctx!()))?;

        Ok(())
    }

    pub async fn time(&mut self) -> io::Result<f64> {
        info!("Method: time");

        retry!(self, self.client.time(ctx!()))
    }

    // Core
    pub async fn prefilter<I: Into<String>>(&mut self, input: I) -> io::Result<Result<String>> {
        info!("Method: prefilter");

        let input = input.into();

        retry!(self, self.client.prefilter(ctx!(), input.clone()))
    }

    pub async fn parse<I: Into<String>>(&mut self, input: I) -> io::Result<Result<Value>> {
        info!("Method: parse");

        let input = input.into();

        retry!(self, self.client.parse(ctx!(), input.clone()))
    }

    pub async fn render<I: Into<String>>(
        &mut self,
        page_info: PageInfoOwned,
        input: I,
    ) -> io::Result<Result<HtmlOutput>> {
        info!("Method: render");

        let input = input.into();

        retry!(
            self,
            self.client.render(ctx!(), page_info.clone(), input.clone()),
        )
    }
}
