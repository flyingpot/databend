// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use std::sync::Arc;

use anyhow::Result;
use common_datavalues::DataSchemaRef;

use crate::ExpressionPlan;
use crate::PlanNode;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SortPlan {
    /// The expression to sort on
    pub order_by: Vec<ExpressionPlan>,
    /// The logical plan
    pub input: Arc<PlanNode>,
}

impl SortPlan {
    pub fn schema(&self) -> DataSchemaRef {
        self.input.schema()
    }

    pub fn input(&self) -> Arc<PlanNode> {
        self.input.clone()
    }

    pub fn set_input(&mut self, input: &PlanNode) -> Result<()> {
        self.input = Arc::new(input.clone());
        Ok(())
    }
}
