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

use std::sync::Arc;

use common_exception::Result;
use common_management::RoleApi;
use common_meta_app::principal::GrantObjectByID;
use common_sql::plans::DropDatabasePlan;
use common_storages_share::save_share_spec;
use common_users::UserApiProvider;

use crate::interpreters::Interpreter;
use crate::pipelines::PipelineBuildResult;
use crate::sessions::QueryContext;
use crate::sessions::TableContext;

pub struct DropDatabaseInterpreter {
    ctx: Arc<QueryContext>,
    plan: DropDatabasePlan,
}

impl DropDatabaseInterpreter {
    pub fn try_create(ctx: Arc<QueryContext>, plan: DropDatabasePlan) -> Result<Self> {
        Ok(DropDatabaseInterpreter { ctx, plan })
    }
}

#[async_trait::async_trait]
impl Interpreter for DropDatabaseInterpreter {
    fn name(&self) -> &str {
        "DropDatabaseInterpreter"
    }

    #[async_backtrace::framed]
    async fn execute2(&self) -> Result<PipelineBuildResult> {
        let tenant = self.ctx.get_tenant();
        let catalog = self.ctx.get_catalog(&self.plan.catalog).await?;
        let role_api = UserApiProvider::instance().get_role_api_client(&tenant)?;

        // unset the ownership of the database, the database may not exists.
        let db = catalog.get_database(&tenant, &self.plan.database).await;
        if let Ok(db) = db {
            role_api
                .drop_ownership(&GrantObjectByID::Database {
                    catalog_name: self.plan.catalog.clone(),
                    db_id: db.get_db_info().ident.db_id,
                })
                .await?;
        }

        // actual drop database
        let resp = catalog.drop_database(self.plan.clone().into()).await?;

        // handle share cleanups with the DropDatabaseReply
        if let Some(spec_vec) = resp.spec_vec {
            let mut share_table_into = Vec::with_capacity(spec_vec.len());
            for share_spec in &spec_vec {
                share_table_into.push((share_spec.name.clone(), None));
            }

            save_share_spec(
                &self.ctx.get_tenant(),
                self.ctx.get_data_operator()?.operator(),
                Some(spec_vec),
                Some(share_table_into),
            )
            .await?;
        }

        Ok(PipelineBuildResult::create())
    }
}
