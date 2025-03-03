// Copyright 2021 Datafuse Labs
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

use lazy_static::lazy_static;

use crate::register_counter;
use crate::register_counter_family;
use crate::Counter;
use crate::Family;
use crate::VecLabels;

lazy_static! {
    static ref QUERY_HTTP_REQUESTS_COUNT: Family<VecLabels, Counter> =
        register_counter_family("query_http_requests_count");
    static ref QUERY_HTTP_SLOW_REQUESTS_COUNT: Family<VecLabels, Counter> =
        register_counter_family("query_http_slow_requests_count");
    static ref QUERY_HTTP_RESPONSE_ERRORS_COUNT: Family<VecLabels, Counter> =
        register_counter_family("query_http_response_errors_count");
    static ref QUERY_HTTP_RESPONSE_PANICS_COUNT: Counter =
        register_counter("query_http_response_panics_count");
}

pub fn metrics_incr_http_request_count(method: String, api: String, status: String) {
    let labels = vec![("method", method), ("api", api), ("status", status)];
    QUERY_HTTP_REQUESTS_COUNT.get_or_create(&labels).inc();
}

pub fn metrics_incr_http_slow_request_count(method: String, api: String, status: String) {
    let labels = vec![("method", method), ("api", api), ("status", status)];
    QUERY_HTTP_SLOW_REQUESTS_COUNT.get_or_create(&labels).inc();
}

pub fn metrics_incr_http_response_errors_count(err: String, code: u16) {
    let labels = vec![("err", err), ("code", code.to_string())];
    QUERY_HTTP_RESPONSE_ERRORS_COUNT
        .get_or_create(&labels)
        .inc();
}

pub fn metrics_incr_http_response_panics_count() {
    QUERY_HTTP_RESPONSE_PANICS_COUNT.inc();
}
