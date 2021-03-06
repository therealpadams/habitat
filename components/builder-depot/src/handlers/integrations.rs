// Copyright (c) 2017 Chef Software Inc. and/or applicable contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::HashMap;

use bldr_core;
use bodyparser;
use http_gateway::http::controller::*;
use http_gateway::http::helpers;
use iron::status::{self, Status};

use protocol::originsrv::*;
use protocol::net::{NetOk, ErrCode};
use persistent;
use router::Router;

use super::super::server::check_origin_access;
use DepotUtil;

pub fn encrypt(req: &mut Request, content: &str) -> Result<String, Status> {
    let lock = req.get::<persistent::State<DepotUtil>>().expect(
        "depot not found",
    );
    let depot = lock.read().expect("depot read lock is poisoned");

    bldr_core::integrations::encrypt(&depot.config.key_dir, content)
        .map_err(|_| status::InternalServerError)
}

pub fn validate_params(
    req: &mut Request,
    expected_params: &[&str],
) -> Result<HashMap<String, String>, Status> {
    let mut res = HashMap::new();
    // Get the expected params
    {
        let params = req.extensions.get::<Router>().unwrap();

        if expected_params.iter().any(|p| params.find(p).is_none()) {
            return Err(status::BadRequest);
        }

        for p in expected_params {
            res.insert(p.to_string(), params.find(p).unwrap().to_string());
        }
    }
    // Check that we have origin access
    {
        let session_id = {
            req.extensions.get::<Authenticated>().unwrap().get_id()
        };
        if !check_origin_access(req, session_id, &res["origin"])
            .map_err(|_| status::InternalServerError)?
        {
            debug!(
                "Failed origin access check, session: {}, origin: {}",
                session_id,
                &res["origin"]
            );
            return Err(status::Forbidden);
        }
    }
    Ok(res)
}

pub fn fetch_origin_integration_names(req: &mut Request) -> IronResult<Response> {
    let params = match validate_params(req, &["origin", "integration"]) {
        Ok(p) => p,
        Err(st) => return Ok(Response::with(st)),
    };

    let mut request = OriginIntegrationGetNames::new();
    request.set_origin(params["origin"].clone());
    request.set_integration(params["integration"].clone());
    match route_message::<OriginIntegrationGetNames, OriginIntegrationNames>(req, &request) {
        Ok(integration) => {
            let mut response = render_json(status::Ok, &integration);
            helpers::dont_cache_response(&mut response);
            Ok(response)
        }
        Err(err) => {
            match err.get_code() {
                ErrCode::ENTITY_NOT_FOUND => Ok(Response::with((status::NotFound))),
                _ => {
                    error!("create_integration:1, err={:?}", err);
                    Ok(Response::with(status::InternalServerError))
                }
            }
        }
    }
}

pub fn create_origin_integration(req: &mut Request) -> IronResult<Response> {
    let params = match validate_params(req, &["origin", "integration", "name"]) {
        Ok(p) => p,
        Err(st) => return Ok(Response::with(st)),
    };

    let body = req.get::<bodyparser::Json>();
    match body {
        Ok(Some(_)) => (),
        Ok(None) => {
            warn!("create_origin_integration: Empty body in request");
            return Ok(Response::with(status::BadRequest));
        }
        Err(e) => {
            warn!("create_origin_integration, Error parsing body: {:?}", e);
            return Ok(Response::with(status::BadRequest));
        }
    };

    let mut oi = OriginIntegration::new();
    oi.set_origin(params["origin"].clone());
    oi.set_integration(params["integration"].clone());
    oi.set_name(params["name"].clone());

    // We know body exists and is valid, non-empty JSON, so we can unwrap safely
    let json_body = req.get::<bodyparser::Raw>().unwrap().unwrap();

    match encrypt(req, &json_body) {
        Ok(encrypted) => oi.set_body(encrypted),
        Err(st) => return Ok(Response::with(st)),
    }

    let mut request = OriginIntegrationCreate::new();
    request.set_integration(oi);

    match route_message::<OriginIntegrationCreate, NetOk>(req, &request) {
        Ok(_) => Ok(Response::with(status::NoContent)),
        Err(err) => {
            if err.get_code() == ErrCode::ENTITY_CONFLICT {
                warn!("Failed to create integration as it already exists");
                Ok(Response::with(status::Conflict))
            } else {
                error!("create_integration:1, err={:?}", err);
                Ok(Response::with(status::InternalServerError))
            }
        }
    }
}

pub fn delete_origin_integration(req: &mut Request) -> IronResult<Response> {
    let params = match validate_params(req, &["origin", "integration", "name"]) {
        Ok(p) => p,
        Err(st) => return Ok(Response::with(st)),
    };

    let mut oi = OriginIntegration::new();
    oi.set_origin(params["origin"].clone());
    oi.set_integration(params["integration"].clone());
    oi.set_name(params["name"].clone());

    let mut request = OriginIntegrationDelete::new();
    request.set_integration(oi);

    match route_message::<OriginIntegrationDelete, NetOk>(req, &request) {
        Ok(_) => Ok(Response::with(status::NoContent)),
        Err(err) => {
            error!("delete_integration:1, err={:?}", err);
            Ok(Response::with(status::InternalServerError))
        }
    }
}
