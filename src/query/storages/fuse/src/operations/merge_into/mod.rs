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

mod mutator;
mod processors;

pub use mutator::MatchedAggregator;
pub use processors::MatchedSplitProcessor;
pub use processors::MergeIntoNotMatchedProcessor;
pub use processors::MergeIntoSplitProcessor;
pub use processors::MixRowNumberKindAndLog;
pub use processors::RowNumberAndLogSplitProcessor;
pub use processors::TransformAddRowNumberColumnProcessor;
pub use processors::TransformDistributedMergeIntoBlockDeserialize;
pub use processors::TransformDistributedMergeIntoBlockSerialize;
