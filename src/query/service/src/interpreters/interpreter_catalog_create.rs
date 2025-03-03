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

use common_catalog::catalog::CatalogManager;
use common_config::GlobalConfig;
use common_exception::ErrorCode;
use common_exception::Result;
use common_meta_app::schema::CatalogId;
use common_meta_app::schema::CatalogInfo;
use common_meta_app::schema::CatalogMeta;
use common_meta_app::schema::CatalogNameIdent;
use common_meta_app::schema::CatalogOption;
use common_sql::plans::CreateCatalogPlan;
use common_storages_fuse::TableContext;
use log::debug;

use super::Interpreter;
use crate::pipelines::PipelineBuildResult;
use crate::sessions::QueryContext;

#[derive(Debug)]
pub struct CreateCatalogInterpreter {
    ctx: Arc<QueryContext>,
    plan: CreateCatalogPlan,
}

impl CreateCatalogInterpreter {
    pub fn try_create(ctx: Arc<QueryContext>, plan: CreateCatalogPlan) -> Result<Self> {
        Ok(CreateCatalogInterpreter { ctx, plan })
    }
}

#[async_trait::async_trait]
impl Interpreter for CreateCatalogInterpreter {
    fn name(&self) -> &str {
        "CreateCatalogInterpreter"
    }

    #[minitrace::trace]
    #[async_backtrace::framed]
    async fn execute2(&self) -> Result<PipelineBuildResult> {
        debug!("ctx.id" = self.ctx.get_id().as_str(); "create_catalog_execute");

        if let CatalogOption::Iceberg(opt) = &self.plan.meta.catalog_option {
            if !opt.storage_params.is_secure() && !GlobalConfig::instance().storage.allow_insecure {
                return Err(ErrorCode::CatalogNotSupported(
                    "Accessing insecure storage in not allowed by configuration",
                ));
            }
        }

        let catalog_manager = CatalogManager::instance();

        // Build and check if catalog is valid.
        let ctl = catalog_manager
            .build_catalog(&CatalogInfo {
                id: CatalogId::default(),
                name_ident: CatalogNameIdent {
                    tenant: self.plan.tenant.clone(),
                    catalog_name: self.plan.catalog.clone(),
                },
                meta: CatalogMeta {
                    catalog_option: self.plan.meta.catalog_option.clone(),
                    created_on: chrono::Utc::now(),
                },
            })
            .map_err(|err| err.add_message("Error creating catalog."))?;

        // list databases to check if the catalog is valid.
        let _ = ctl.list_databases(&self.plan.tenant).await.map_err(|err| {
            err.add_message("Catalog creation failed. Check your parameter values.")
        })?;

        catalog_manager
            .create_catalog(self.plan.clone().into())
            .await?;

        Ok(PipelineBuildResult::create())
    }
}
