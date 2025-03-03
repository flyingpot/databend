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

use std::collections::HashMap;
use std::sync::Arc;

use common_ast::ast::WindowDefinition;
use common_ast::ast::WindowSpec;
use common_exception::ErrorCode;
use common_exception::Result;
use common_exception::Span;

use super::select::SelectList;
use crate::binder::ColumnBindingBuilder;
use crate::optimizer::SExpr;
use crate::plans::AggregateFunction;
use crate::plans::BoundColumnRef;
use crate::plans::CastExpr;
use crate::plans::FunctionCall;
use crate::plans::LagLeadFunction;
use crate::plans::LambdaFunc;
use crate::plans::NthValueFunction;
use crate::plans::ScalarExpr;
use crate::plans::ScalarItem;
use crate::plans::UDFServerCall;
use crate::plans::Window;
use crate::plans::WindowFunc;
use crate::plans::WindowFuncFrame;
use crate::plans::WindowFuncType;
use crate::plans::WindowOrderBy;
use crate::BindContext;
use crate::Binder;
use crate::IndexType;
use crate::MetadataRef;
use crate::Visibility;

impl Binder {
    #[async_backtrace::framed]
    pub(super) async fn bind_window_function(
        &mut self,
        window_info: &WindowFunctionInfo,
        child: SExpr,
    ) -> Result<SExpr> {
        let window_plan = Window {
            span: window_info.span,
            index: window_info.index,
            function: window_info.func.clone(),
            arguments: window_info.arguments.clone(),
            partition_by: window_info.partition_by_items.clone(),
            order_by: window_info.order_by_items.clone(),
            frame: window_info.frame.clone(),
        };

        Ok(SExpr::create_unary(
            Arc::new(window_plan.into()),
            Arc::new(child),
        ))
    }

    pub(super) fn analyze_window_definition(
        &self,
        bind_context: &mut BindContext,
        window_list: &Option<Vec<WindowDefinition>>,
    ) -> Result<()> {
        if window_list.is_none() {
            return Ok(());
        }
        let window_list = window_list.as_ref().unwrap();
        let (window_specs, mut resolved_window_specs) =
            self.extract_window_definitions(window_list.as_slice())?;

        let window_definitions = dashmap::DashMap::with_capacity(window_specs.len());

        for (name, spec) in window_specs.iter() {
            let new_spec = Self::rewrite_inherited_window_spec(
                spec,
                &window_specs,
                &mut resolved_window_specs,
            )?;
            window_definitions.insert(name.clone(), new_spec);
        }
        bind_context.window_definitions = window_definitions;
        Ok(())
    }

    fn extract_window_definitions(
        &self,
        window_list: &[WindowDefinition],
    ) -> Result<(HashMap<String, WindowSpec>, HashMap<String, WindowSpec>)> {
        let mut window_specs = HashMap::new();
        let mut resolved_window_specs = HashMap::new();
        for window in window_list {
            window_specs.insert(window.name.name.clone(), window.spec.clone());
            if window.spec.existing_window_name.is_none() {
                resolved_window_specs.insert(window.name.name.clone(), window.spec.clone());
            }
        }
        Ok((window_specs, resolved_window_specs))
    }

    fn rewrite_inherited_window_spec(
        window_spec: &WindowSpec,
        window_list: &HashMap<String, WindowSpec>,
        resolved_window: &mut HashMap<String, WindowSpec>,
    ) -> Result<WindowSpec> {
        if window_spec.existing_window_name.is_some() {
            let referenced_name = window_spec
                .existing_window_name
                .as_ref()
                .unwrap()
                .name
                .clone();
            // check if spec is resolved first, so that we no need to resolve again.
            let referenced_window_spec = {
                resolved_window.get(&referenced_name).unwrap_or(
                    window_list
                        .get(&referenced_name)
                        .ok_or_else(|| ErrorCode::SyntaxException("Referenced window not found"))?,
                )
            }
            .clone();

            let resolved_spec = match referenced_window_spec.existing_window_name.clone() {
                Some(_) => Self::rewrite_inherited_window_spec(
                    &referenced_window_spec,
                    window_list,
                    resolved_window,
                )?,
                None => referenced_window_spec.clone(),
            };

            // add to resolved.
            resolved_window.insert(referenced_name, resolved_spec.clone());

            // check semantic
            if !window_spec.partition_by.is_empty() {
                return Err(ErrorCode::SemanticError(
                    "WINDOW specification with named WINDOW reference cannot specify PARTITION BY",
                ));
            }
            if !window_spec.order_by.is_empty() && !resolved_spec.order_by.is_empty() {
                return Err(ErrorCode::SemanticError(
                    "Cannot specify ORDER BY if referenced named WINDOW specifies ORDER BY",
                ));
            }
            if resolved_spec.window_frame.is_some() {
                return Err(ErrorCode::SemanticError(
                    "Cannot reference named WINDOW containing frame specification",
                ));
            }

            // resolve referenced window
            let mut partition_by = window_spec.partition_by.clone();
            if !resolved_spec.partition_by.is_empty() {
                partition_by = resolved_spec.partition_by.clone();
            }

            let mut order_by = window_spec.order_by.clone();
            if order_by.is_empty() && !resolved_spec.order_by.is_empty() {
                order_by = resolved_spec.order_by.clone();
            }

            let mut window_frame = window_spec.window_frame.clone();
            if window_frame.is_none() && resolved_spec.window_frame.is_some() {
                window_frame = resolved_spec.window_frame;
            }

            // replace with new window spec
            let new_window_spec = WindowSpec {
                existing_window_name: None,
                partition_by,
                order_by,
                window_frame,
            };
            Ok(new_window_spec)
        } else {
            Ok(window_spec.clone())
        }
    }
}

#[derive(Default, Clone, PartialEq, Eq, Debug)]
pub struct WindowInfo {
    pub window_functions: Vec<WindowFunctionInfo>,
    pub window_functions_map: HashMap<String, usize>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WindowFunctionInfo {
    pub span: Span,
    pub index: IndexType,
    pub func: WindowFuncType,
    pub arguments: Vec<ScalarItem>,
    pub partition_by_items: Vec<ScalarItem>,
    pub order_by_items: Vec<WindowOrderByInfo>,
    pub frame: WindowFuncFrame,
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct WindowOrderByInfo {
    pub order_by_item: ScalarItem,
    pub asc: Option<bool>,
    pub nulls_first: Option<bool>,
}

pub(super) struct WindowRewriter<'a> {
    pub bind_context: &'a mut BindContext,
    pub metadata: MetadataRef,
    // While analyzing in-window aggregate function, we can replace it with a BoundColumnRef
    in_window: bool,
}

impl<'a> WindowRewriter<'a> {
    pub fn new(bind_context: &'a mut BindContext, metadata: MetadataRef) -> Self {
        Self {
            bind_context,
            metadata,
            in_window: false,
        }
    }

    pub fn visit(&mut self, scalar: &ScalarExpr) -> Result<ScalarExpr> {
        match scalar {
            ScalarExpr::BoundColumnRef(_) => Ok(scalar.clone()),
            ScalarExpr::ConstantExpr(_) => Ok(scalar.clone()),
            ScalarExpr::FunctionCall(func) => {
                let new_args = func
                    .arguments
                    .iter()
                    .map(|arg| self.visit(arg))
                    .collect::<Result<Vec<_>>>()?;
                Ok(FunctionCall {
                    span: func.span,
                    func_name: func.func_name.clone(),
                    params: func.params.clone(),
                    arguments: new_args,
                }
                .into())
            }
            ScalarExpr::CastExpr(cast) => Ok(CastExpr {
                span: cast.span,
                is_try: cast.is_try,
                argument: Box::new(self.visit(&cast.argument)?),
                target_type: cast.target_type.clone(),
            }
            .into()),

            // TODO(leiysky): should we recursively process subquery here?
            ScalarExpr::SubqueryExpr(_) => Ok(scalar.clone()),

            ScalarExpr::AggregateFunction(agg_func) => {
                if self.in_window {
                    if let Some(index) = self
                        .bind_context
                        .aggregate_info
                        .aggregate_functions_map
                        .get(&agg_func.display_name)
                    {
                        let agg = &self.bind_context.aggregate_info.aggregate_functions[*index];
                        let column_binding = ColumnBindingBuilder::new(
                            agg_func.display_name.clone(),
                            agg.index,
                            agg_func.return_type.clone(),
                            Visibility::Visible,
                        )
                        .build();
                        Ok(BoundColumnRef {
                            span: None,
                            column: column_binding,
                        }
                        .into())
                    } else {
                        Err(ErrorCode::BadArguments("Invalid window function argument"))
                    }
                } else {
                    let new_args = agg_func
                        .args
                        .iter()
                        .map(|arg| self.visit(arg))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(AggregateFunction {
                        func_name: agg_func.func_name.clone(),
                        distinct: agg_func.distinct,
                        params: agg_func.params.clone(),
                        args: new_args,
                        return_type: agg_func.return_type.clone(),
                        display_name: agg_func.display_name.clone(),
                    }
                    .into())
                }
            }

            ScalarExpr::WindowFunction(window) => {
                self.in_window = true;
                let scalar = self.replace_window_function(window)?;
                self.in_window = false;
                Ok(scalar)
            }
            ScalarExpr::UDFServerCall(udf) => {
                let new_args = udf
                    .arguments
                    .iter()
                    .map(|arg| self.visit(arg))
                    .collect::<Result<Vec<_>>>()?;
                Ok(UDFServerCall {
                    span: udf.span,
                    func_name: udf.func_name.clone(),
                    display_name: udf.display_name.clone(),
                    server_addr: udf.server_addr.clone(),
                    arg_types: udf.arg_types.clone(),
                    return_type: udf.return_type.clone(),
                    arguments: new_args,
                }
                .into())
            }
            ScalarExpr::LambdaFunction(lambda_func) => {
                let new_args = lambda_func
                    .args
                    .iter()
                    .map(|arg| self.visit(arg))
                    .collect::<Result<Vec<_>>>()?;
                Ok(LambdaFunc {
                    span: lambda_func.span,
                    func_name: lambda_func.func_name.clone(),
                    display_name: lambda_func.display_name.clone(),
                    args: new_args,
                    params: lambda_func.params.clone(),
                    lambda_expr: lambda_func.lambda_expr.clone(),
                    return_type: lambda_func.return_type.clone(),
                }
                .into())
            }
        }
    }

    fn replace_window_function(&mut self, window: &WindowFunc) -> Result<ScalarExpr> {
        let mut replaced_partition_items: Vec<ScalarExpr> =
            Vec::with_capacity(window.partition_by.len());
        let mut replaced_order_by_items: Vec<WindowOrderBy> =
            Vec::with_capacity(window.order_by.len());
        let mut window_args = vec![];

        let window_func_name = window.func.func_name();
        let func = match &window.func {
            WindowFuncType::Aggregate(agg) => {
                // resolve aggregate function args in window function.
                let mut replaced_args: Vec<ScalarExpr> = Vec::with_capacity(agg.args.len());
                for (i, arg) in agg.args.iter().enumerate() {
                    let arg = self.visit(arg)?;
                    let name = format!("{window_func_name}_arg_{i}");
                    let replaced_arg = self.replace_expr(&name, &arg)?;
                    window_args.push(ScalarItem {
                        index: replaced_arg.column.index,
                        scalar: arg,
                    });
                    replaced_args.push(replaced_arg.into());
                }
                WindowFuncType::Aggregate(AggregateFunction {
                    display_name: agg.display_name.clone(),
                    func_name: agg.func_name.clone(),
                    distinct: agg.distinct,
                    params: agg.params.clone(),
                    args: replaced_args,
                    return_type: agg.return_type.clone(),
                })
            }
            WindowFuncType::LagLead(ll) => {
                let (new_arg, new_default) =
                    self.replace_lag_lead_args(&mut window_args, &window_func_name, ll)?;

                WindowFuncType::LagLead(LagLeadFunction {
                    is_lag: ll.is_lag,
                    arg: Box::new(new_arg),
                    offset: ll.offset,
                    default: new_default,
                    return_type: ll.return_type.clone(),
                })
            }
            WindowFuncType::NthValue(func) => {
                let arg = self.visit(&func.arg)?;
                let name = format!("{window_func_name}_arg");
                let replaced_arg = self.replace_expr(&name, &arg)?;
                window_args.push(ScalarItem {
                    index: replaced_arg.column.index,
                    scalar: arg,
                });
                WindowFuncType::NthValue(NthValueFunction {
                    n: func.n,
                    arg: Box::new(replaced_arg.into()),
                    return_type: func.return_type.clone(),
                })
            }
            func => func.clone(),
        };

        // resolve partition by
        let mut partition_by_items = vec![];
        for (i, part) in window.partition_by.iter().enumerate() {
            let part = self.visit(part)?;
            let name = format!("{window_func_name}_part_{i}");
            let replaced_part = self.replace_expr(&name, &part)?;
            partition_by_items.push(ScalarItem {
                index: replaced_part.column.index,
                scalar: part,
            });
            replaced_partition_items.push(replaced_part.into());
        }

        // resolve order by
        let mut order_by_items = vec![];
        for (i, order) in window.order_by.iter().enumerate() {
            let order_expr = self.visit(&order.expr)?;
            let name = format!("{window_func_name}_order_{i}");
            let replaced_order = self.replace_expr(&name, &order_expr)?;
            order_by_items.push(WindowOrderByInfo {
                order_by_item: ScalarItem {
                    index: replaced_order.column.index,
                    scalar: order_expr,
                },
                asc: order.asc,
                nulls_first: order.nulls_first,
            });
            replaced_order_by_items.push(WindowOrderBy {
                expr: replaced_order.into(),
                asc: order.asc,
                nulls_first: order.nulls_first,
            });
        }

        let index = self
            .metadata
            .write()
            .add_derived_column(window.display_name.clone(), window.func.return_type());

        // create window info
        let window_info = WindowFunctionInfo {
            span: window.span,
            index,
            func: func.clone(),
            arguments: window_args,
            partition_by_items,
            order_by_items,
            frame: window.frame.clone(),
        };

        let window_infos = &mut self.bind_context.windows;
        // push window info to BindContext
        window_infos.window_functions.push(window_info);
        window_infos.window_functions_map.insert(
            window.display_name.clone(),
            window_infos.window_functions.len() - 1,
        );

        let replaced_window = WindowFunc {
            span: window.span,
            display_name: window.display_name.clone(),
            func,
            partition_by: replaced_partition_items,
            order_by: replaced_order_by_items,
            frame: window.frame.clone(),
        };

        Ok(replaced_window.into())
    }

    fn replace_lag_lead_args(
        &mut self,
        window_args: &mut Vec<ScalarItem>,
        window_func_name: &String,
        f: &LagLeadFunction,
    ) -> Result<(ScalarExpr, Option<Box<ScalarExpr>>)> {
        let arg = self.visit(&f.arg)?;
        let name = format!("{window_func_name}_arg");
        let replaced_arg = self.replace_expr(&name, &arg)?;
        window_args.push(ScalarItem {
            scalar: arg,
            index: replaced_arg.column.index,
        });
        let new_default = match &f.default {
            None => None,
            Some(d) => {
                let d = self.visit(d)?;
                let name = format!("{window_func_name}_default_value");
                let replaced_default = self.replace_expr(&name, &d)?;
                window_args.push(ScalarItem {
                    scalar: d,
                    index: replaced_default.column.index,
                });
                Some(Box::new(replaced_default.into()))
            }
        };
        Ok((replaced_arg.into(), new_default))
    }

    fn replace_expr(&self, name: &str, arg: &ScalarExpr) -> Result<BoundColumnRef> {
        if let ScalarExpr::BoundColumnRef(col) = &arg {
            Ok(col.clone())
        } else {
            let ty = arg.data_type()?;
            let index = self
                .metadata
                .write()
                .add_derived_column(name.to_string(), ty.clone());

            // Generate a ColumnBinding for each argument of aggregates
            let column = ColumnBindingBuilder::new(
                name.to_string(),
                index,
                Box::new(ty),
                Visibility::Visible,
            )
            .build();
            Ok(BoundColumnRef {
                span: arg.span(),
                column,
            })
        }
    }
}

impl Binder {
    /// Analyze windows in select clause, this will rewrite window functions.
    /// See [`WindowRewriter`] for more details.
    pub(crate) fn analyze_window(
        &mut self,
        bind_context: &mut BindContext,
        select_list: &mut SelectList,
    ) -> Result<()> {
        for item in select_list.items.iter_mut() {
            let mut rewriter = WindowRewriter::new(bind_context, self.metadata.clone());
            let new_scalar = rewriter.visit(&item.scalar)?;
            item.scalar = new_scalar;
        }

        Ok(())
    }
}
