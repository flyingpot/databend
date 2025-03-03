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

use chrono_tz::Tz;

#[derive(Clone)]
pub struct InputCommonSettings {
    pub true_bytes: Vec<u8>,
    pub false_bytes: Vec<u8>,
    pub null_if: Vec<Vec<u8>>,
    pub nan_bytes: Vec<u8>,
    pub inf_bytes: Vec<u8>,
    pub timezone: Tz,
    pub disable_variant_check: bool,
}

#[derive(Clone)]
pub struct OutputCommonSettings {
    pub true_bytes: Vec<u8>,
    pub false_bytes: Vec<u8>,
    pub null_bytes: Vec<u8>,
    pub nan_bytes: Vec<u8>,
    pub inf_bytes: Vec<u8>,
    pub timezone: Tz,
}
