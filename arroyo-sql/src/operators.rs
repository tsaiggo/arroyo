use std::time::Duration;

use crate::{
    expressions::{AggregationExpression, Aggregator, Column, Expression},
    schemas::window_type_def,
    types::{StructDef, StructField, TypeDef},
};
use anyhow::Result;
use arrow_schema::DataType;
use arroyo_datastream::WindowType;
use datafusion_expr::type_coercion::aggregates::{avg_return_type, sum_return_type};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, parse_str, Ident, LitInt};

#[derive(Debug, Clone)]
pub struct Projection {
    pub field_names: Vec<Column>,
    pub field_computations: Vec<Expression>,
}

impl Projection {
    pub fn without_window(self) -> Self {
        let (field_names, field_computations) = self
            .field_computations
            .into_iter()
            .enumerate()
            .filter_map(|(i, computation)| {
                let field_name = self.field_names[i].clone();
                if window_type_def() == computation.return_type() {
                    None
                } else {
                    Some((field_name, computation))
                }
            })
            .unzip();
        Self {
            field_names,
            field_computations,
        }
    }

    pub fn output_struct(&self) -> StructDef {
        let fields = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, computation)| {
                let field_name = self.field_names[i].clone();
                let field_type = computation.return_type();
                StructField {
                    alias: field_name.relation,
                    name: field_name.name,
                    data_type: field_type,
                }
            })
            .collect();
        StructDef { name: None, fields }
    }
    pub fn to_truncated_syn_expression(&self, terms: usize) -> syn::Expr {
        let assignments: Vec<_> = self
            .field_computations
            .iter()
            .enumerate()
            .take(terms)
            .map(|(i, field)| {
                let field_name = self.field_names[i].clone();
                let name = field_name.name;
                let alias = field_name.relation;
                let data_type = field.return_type();
                let field_ident = StructField {
                    name,
                    alias,
                    data_type,
                }
                .field_ident();
                let expr = field.to_syn_expression();
                quote!(#field_ident : #expr)
            })
            .collect();
        let output_type = self.truncated_return_type(terms).get_type();
        parse_quote!(
                #output_type {
                    #(#assignments)
                    ,*
                }
        )
    }

    pub fn truncated_return_type(&self, terms: usize) -> StructDef {
        let fields = self
            .field_computations
            .iter()
            .enumerate()
            .take(terms)
            .map(|(i, computation)| {
                let field_name = self.field_names[i].clone();
                let field_type = computation.return_type();
                StructField {
                    alias: field_name.relation,
                    name: field_name.name,
                    data_type: field_type,
                }
            })
            .collect();
        StructDef { name: None, fields }
    }

    pub fn to_syn_expression(&self) -> syn::Expr {
        let assignments: Vec<_> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let field_name = self.field_names[i].clone();
                let name = field_name.name;
                let alias = field_name.relation;
                let data_type = field.return_type();
                let field_ident = StructField {
                    name,
                    alias,
                    data_type,
                }
                .field_ident();
                let expr = field.to_syn_expression();
                quote!(#field_ident : #expr)
            })
            .collect();
        let output_type = self.return_type().return_type();
        parse_quote!(
                #output_type {
                    #(#assignments)
                    ,*
                }
        )
    }

    fn return_type(&self) -> TypeDef {
        TypeDef::StructDef(self.output_struct(), false)
    }
}

#[derive(Debug, Clone)]
pub struct AggregateProjection {
    pub field_names: Vec<Column>,
    pub field_computations: Vec<AggregationExpression>,
}

impl AggregateProjection {
    pub fn output_struct(&self) -> StructDef {
        let fields = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, computation)| {
                let field_name = self.field_names[i].clone();
                let field_type = computation.return_type();
                StructField {
                    alias: field_name.relation,
                    name: field_name.name,
                    data_type: field_type,
                }
            })
            .collect();
        StructDef { name: None, fields }
    }

    pub fn to_syn_expression(&self) -> syn::Expr {
        let assignments: Vec<_> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let field_name = self.field_names[i].clone();
                let name = field_name.name;
                let alias = field_name.relation;
                let data_type = field_computation.return_type();
                let expr = field_computation.to_syn_expression();
                let field_ident = StructField {
                    name,
                    alias,
                    data_type,
                }
                .field_ident();
                quote!(#field_ident: #expr)
            })
            .collect();
        let output_type = self.return_type().return_type();
        parse_quote!(
            {
                #output_type {
                    #(#assignments)
                    ,*
                }
            }
        )
    }

    fn return_type(&self) -> TypeDef {
        TypeDef::StructDef(self.output_struct(), false)
    }
}

#[derive(Debug, Clone)]
pub enum GroupByKind {
    Basic,
    WindowOutput {
        index: usize,
        column: Column,
        window_type: WindowType,
    },
}

impl GroupByKind {
    pub fn output_struct(&self, key_struct: &StructDef, aggregate_struct: &StructDef) -> StructDef {
        let key_fields = key_struct.fields.len();
        let aggregate_fields = aggregate_struct.fields.len();
        match self {
            GroupByKind::WindowOutput {
                index,
                column,
                window_type: _,
            } => {
                let fields = (0..(key_fields + aggregate_fields + 1))
                    .map(|i| {
                        if i < key_fields + 1 {
                            if i == *index {
                                StructField {
                                    name: column.name.clone(),
                                    alias: column.relation.clone(),
                                    data_type: window_type_def(),
                                }
                            } else if i < *index {
                                key_struct.fields[i].clone()
                            } else {
                                key_struct.fields[i - 1].clone()
                            }
                        } else {
                            aggregate_struct.fields[i - key_fields - 1].clone()
                        }
                    })
                    .collect();
                StructDef { name: None, fields }
            }
            GroupByKind::Basic => {
                let fields = (0..(key_fields + aggregate_fields))
                    .map(|i| {
                        if i < key_fields {
                            key_struct.fields[i].clone()
                        } else {
                            aggregate_struct.fields[i - key_fields].clone()
                        }
                    })
                    .collect();
                StructDef { name: None, fields }
            }
        }
    }

    pub fn to_syn_expression(
        &self,
        key_struct: &StructDef,
        aggregate_struct: &StructDef,
    ) -> syn::Expr {
        let mut assignments: Vec<_> = vec![];

        key_struct.fields.iter().for_each(|field| {
            let field_name: Ident = format_ident!("{}", field.field_name());
            assignments.push(quote!(#field_name : arg.key.#field_name.clone()));
        });
        aggregate_struct.fields.iter().for_each(|field| {
            let field_name: Ident = format_ident!("{}", field.field_name());
            assignments.push(quote!(#field_name : arg.aggregate.#field_name.clone()));
        });
        let return_struct = self.output_struct(key_struct, aggregate_struct);
        if let GroupByKind::WindowOutput {
            index,
            column: _,
            window_type,
        } = self
        {
            let width = match window_type {
                WindowType::Tumbling { width } | WindowType::Sliding { width, .. } => width,
                WindowType::Instant => &Duration::ZERO,
            };
            let field_name = format_ident!("{}", return_struct.fields[*index].field_name());
            let width_literal: LitInt = parse_str(&width.as_millis().to_string()).unwrap();
            assignments.push(quote!(#field_name: arroyo_types::Window{
                        start_time: arg.timestamp - std::time::Duration::from_millis(#width_literal) + std::time::Duration::from_nanos(1),
                        end_time: arg.timestamp + std::time::Duration::from_nanos(1)}));
        }
        let return_type = return_struct.get_type();
        parse_quote!(
            #return_type {
                    #(#assignments)
                    ,*
                }
        )
    }
}

#[derive(Debug, Clone)]
pub struct TwoPhaseAggregateProjection {
    pub field_names: Vec<Column>,
    pub field_computations: Vec<TwoPhaseAggregation>,
}

impl TryFrom<AggregateProjection> for TwoPhaseAggregateProjection {
    type Error = anyhow::Error;

    fn try_from(aggregate_projection: AggregateProjection) -> Result<Self> {
        Ok(Self {
            field_names: aggregate_projection.field_names,
            field_computations: aggregate_projection
                .field_computations
                .into_iter()
                .map(|computation| computation.try_into())
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl TwoPhaseAggregateProjection {
    pub fn combine_bin_syn_expr(&self) -> syn::Expr {
        let some_assignments: Vec<syn::Expr> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let expr = field_computation.combine_bin_syn_expr();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                parse_quote!({let current_bin = current_bin.#i.clone();
                    let new_bin = arg.#i.clone();  #expr})
            })
            .collect();
        let trailing_comma = self.trailing_comma();
        parse_quote!(
            match current_bin {
                Some(current_bin) => {
                    (#(#some_assignments),*#trailing_comma)
                }
                None => arg.clone()
            }
        )
    }

    pub fn bin_merger_syn_expression(&self) -> syn::Expr {
        let some_assignments: Vec<syn::Expr> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let expr = field_computation.bin_syn_expr();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                parse_quote!({let current_bin = Some(current_bin.#i.clone()); #expr})
            })
            .collect();
        let none_assignments: Vec<syn::Expr> = self
            .field_computations
            .iter()
            .map(|field_computation| {
                let expr = field_computation.bin_syn_expr();
                let bin_type = field_computation.bin_type();
                parse_quote!({let current_bin:Option<#bin_type> = None; #expr})
            })
            .collect();

        let trailing_comma = self.trailing_comma();
        parse_quote!(
            match current_bin {
                Some(current_bin) => {
                    (#(#some_assignments),*#trailing_comma)},
                None => {
                    (#(#none_assignments),*#trailing_comma)}
            }
        )
    }

    pub fn tumbling_aggregation_syn_expression(&self) -> syn::Expr {
        let assignments: Vec<_> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let field_name = self.field_names[i].clone();
                let name = field_name.name;
                let alias = field_name.relation;
                let data_type = field_computation.return_type();
                let expr = field_computation.bin_aggregating_expression();
                let field_ident = StructField {
                    name,
                    alias,
                    data_type,
                }
                .field_ident();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                quote!(#field_ident: {let arg = &arg.#i; #expr})
            })
            .collect();
        let output_type = self.output_struct().get_type();
        parse_quote!(
            {
                #output_type {
                    #(#assignments)
                    ,*
                }
            }
        )
    }
    pub fn sliding_aggregation_syn_expression(&self) -> syn::Expr {
        let assignments: Vec<_> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let field_name = self.field_names[i].clone();
                let name = field_name.name;
                let alias = field_name.relation;
                let data_type = field_computation.return_type();
                let expr = field_computation.to_aggregating_syn_expression();
                let field_ident = StructField {
                    name,
                    alias,
                    data_type,
                }
                .field_ident();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                quote!(#field_ident: {let arg = &arg.1.#i; #expr})
            })
            .collect();
        let output_type = self.output_struct().get_type();
        parse_quote!(
            {
                #output_type {
                    #(#assignments)
                    ,*
                }
            }
        )
    }

    pub fn output_struct(&self) -> StructDef {
        let fields = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, computation)| {
                let field_name = self.field_names[i].clone();
                let field_type = computation.return_type();
                StructField {
                    alias: field_name.relation,
                    name: field_name.name,
                    data_type: field_type,
                }
            })
            .collect();
        StructDef { name: None, fields }
    }

    fn trailing_comma(&self) -> Option<TokenStream> {
        if self.field_computations.len() == 1 {
            Some(quote!(,))
        } else {
            None
        }
    }

    pub(crate) fn memory_add_syn_expression(&self) -> syn::Expr {
        let trailing_comma = self.trailing_comma();
        let some_assignments: Vec<syn::Expr> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let expr = field_computation.memory_add_syn_expr();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                parse_quote!({let current = Some(current.#i);
                    let bin_value = bin_value.#i;
                     #expr})
            })
            .collect();
        let none_assignments: Vec<syn::Expr> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let expr = field_computation.memory_add_syn_expr();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                parse_quote!({let current = None;
                let bin_value = bin_value.#i;
                 #expr})
            })
            .collect();
        parse_quote!(
            match current {
                Some((i, current)) => {
                    (i +1, (#(#some_assignments),*#trailing_comma))},
                None => {
                    (1, (#(#none_assignments),*#trailing_comma))}
            }
        )
    }

    pub(crate) fn memory_remove_syn_expression(&self) -> syn::Expr {
        let removals: Vec<syn::Expr> = self
            .field_computations
            .iter()
            .enumerate()
            .map(|(i, field_computation)| {
                let expr = field_computation.memory_remove_syn_expr();
                let i: syn::Index = parse_str(&i.to_string()).unwrap();
                parse_quote!({let current = current.1.#i;
                    let bin_value = bin_value.#i;
                     #expr.unwrap()})
            })
            .collect();

        let trailing_comma = self.trailing_comma();
        parse_quote!(
            if current.0 == 1 {
                None
            } else {
                Some((current.0 - 1, (#(#removals),*#trailing_comma)))
            }
        )
    }

    pub(crate) fn bin_type(&self) -> syn::Type {
        let trailing_comma = self.trailing_comma();
        let bin_types: Vec<_> = self
            .field_computations
            .iter()
            .map(|computation| computation.bin_type())
            .collect();
        parse_quote!((#(#bin_types),*#trailing_comma))
    }

    pub(crate) fn memory_type(&self) -> syn::Type {
        let trailing_comma = self.trailing_comma();
        let mem_types: Vec<_> = self
            .field_computations
            .iter()
            .map(|computation| computation.mem_type())
            .collect();
        parse_quote!((usize,(#(#mem_types),*#trailing_comma)))
    }
}

#[derive(Debug, Clone)]
pub struct TwoPhaseAggregation {
    pub incoming_expression: Expression,
    pub aggregator: Aggregator,
}

impl TwoPhaseAggregation {
    fn aggregate_type(&self) -> syn::Type {
        self.aggregate_type_def().return_type()
    }

    fn aggregate_type_def(&self) -> TypeDef {
        let incoming_type = self.incoming_expression.return_type();
        let data_type = match incoming_type {
            TypeDef::StructDef(_, _) => unreachable!(),
            TypeDef::DataType(data_type, _) => data_type,
        };
        let aggregate_type = match self.aggregator {
            Aggregator::Count => DataType::Int64,
            Aggregator::Sum | Aggregator::Avg => {
                sum_return_type(&data_type).expect("datafusion should've prevented this")
            }
            Aggregator::Min | Aggregator::Max => data_type,
            Aggregator::CountDistinct => unimplemented!(),
        };
        TypeDef::DataType(aggregate_type, false)
    }

    fn bin_type(&self) -> syn::Type {
        let input_nullable = self.incoming_expression.nullable();
        let aggregate_type = self.aggregate_type();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, _) => parse_quote!(i64),
            (Aggregator::Sum, true) | (Aggregator::Min, true) | (Aggregator::Max, true) => {
                parse_quote!(Option<#aggregate_type>)
            }
            (Aggregator::Sum, false) | (Aggregator::Min, false) | (Aggregator::Max, false) => {
                parse_quote!( #aggregate_type)
            }
            (Aggregator::Avg, true) => parse_quote!(Option<(i64, #aggregate_type)>),
            (Aggregator::Avg, false) => parse_quote!((i64, #aggregate_type)),
            (Aggregator::CountDistinct, _) => unimplemented!(),
        }
    }

    fn combine_bin_syn_expr(&self) -> syn::Expr {
        let input_nullable = self.incoming_expression.nullable();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, _) => parse_quote!({ current_bin + new_bin }),
            (Aggregator::Sum, true) => parse_quote!({
                match (current_bin, new_bin) {
                    (Some(value), Some(addition)) => Some(value + addition),
                    (Some(value), None) => Some(value),
                    (None, Some(addition)) => Some(addition),
                    (None, None) => None,
                }
            }),
            (Aggregator::Sum, false) => parse_quote!({ current_bin + new_bin }),
            (Aggregator::Min, true) => parse_quote!({
                match (current_bin, new_bin) {
                    (Some(value), Some(new_value)) => Some(value.min(new_value)),
                    (Some(value), None) => Some(value),
                    (None, Some(new_value)) => Some(new_value),
                    (None, None) => None,
                }
            }),
            (Aggregator::Min, false) => parse_quote!({ current_bin.min(new_bin) }),
            (Aggregator::Max, true) => parse_quote!({
                match (current_bin, new_bin) {
                    (Some(value), Some(new_value)) => Some(value.max(new_value)),
                    (Some(value), None) => Some(value),
                    (None, Some(new_value)) => Some(new_value),
                    (None, None) => None,
                }
            }),
            (Aggregator::Max, false) => parse_quote!({ current_bin.max(new_bin) }),
            (Aggregator::Avg, true) => parse_quote!({
                match (current_bin, new_bin) {
                    (Some((current_count, current_sum)), Some((new_count, new_sum))) => {
                        Some((current_count + new_count, current_sum + new_sum))
                    }
                    (Some((count, sum)), None) => Some((count, sum)),
                    (None, Some((count, sum))) => Some((count, sum)),
                    (None, None) => None,
                }
            }),
            (Aggregator::Avg, false) => {
                parse_quote!({ (current_bin.0 + new_bin.0, current_bin.1 + new_bin.1) })
            }
            (Aggregator::CountDistinct, _) => unreachable!("no two phase for count distinct"),
        }
    }

    fn bin_syn_expr(&self) -> syn::Expr {
        let expr = self.incoming_expression.to_syn_expression();
        let aggregate_type = self.aggregate_type();
        let input_nullable = self.incoming_expression.nullable();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, true) => parse_quote!({
                let  count = current_bin.unwrap_or(0);
                let addition = if #expr.is_some() {1} else {0};
                count + addition
            }),
            (Aggregator::Count, false) => parse_quote!({ current_bin.unwrap_or(0) + 1 }),
            (Aggregator::Sum, true) => parse_quote!({
                match (current_bin.flatten(), #expr) {
                    (Some(value), Some(addition)) => Some(value + (addition as #aggregate_type)),
                    (Some(value), None) => Some(value),
                    (None, Some(addition)) => Some(addition as #aggregate_type),
                    (None, None) => None,
                }
            }),
            (Aggregator::Sum, false) => parse_quote!({
                match current_bin {
                    Some(value) => value + (#expr as #aggregate_type),
                    None => (#expr as #aggregate_type),
                }
            }),
            (Aggregator::Min, true) => parse_quote!({
                match (current_bin.flatten(), #expr) {
                    (Some(value), Some(new_value)) => Some(value.min(new_value)),
                    (Some(value), None) => Some(value),
                    (None, Some(new_value)) => Some(new_value),
                    (None, None) => None,
                }
            }),
            (Aggregator::Min, false) => parse_quote!({
                match current_bin {
                    Some(value) => value.min(#expr),
                    None => #expr
                }
            }),
            (Aggregator::Max, true) => parse_quote!({
                match (current_bin.flatten(), #expr) {
                    (Some(value), Some(new_value)) => Some(value.max(new_value)),
                    (Some(value), None) => Some(value),
                    (None, Some(new_value)) => Some(new_value),
                    (None, None) => None,
                }
            }),
            (Aggregator::Max, false) => parse_quote!({
                match current_bin {
                    Some(value) => value.max(#expr),
                    None => #expr
                }
            }),
            (Aggregator::Avg, true) => parse_quote!({
                match (current_bin.flatten(), #expr) {
                    (Some((count, sum)), Some(value)) => Some((count + 1, sum + (value as #aggregate_type))),
                    (Some((count, sum)), None) => Some((count, sum)),
                    (None, Some(value)) => Some((1, value as #aggregate_type)),
                    (None, None) => None,
                }
            }),
            (Aggregator::Avg, false) => parse_quote!({
                match current_bin {
                    Some((count, sum)) => (count + 1, sum + (#expr as #aggregate_type)),
                    None => (1, #expr as #aggregate_type)
                }
            }),
            (Aggregator::CountDistinct, _) => unreachable!("no two phase for count distinct"),
        }
    }

    fn mem_type(&self) -> syn::Type {
        let input_nullable = self.incoming_expression.nullable();
        let expr_type = self.aggregate_type();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, _) => parse_quote!((i64, i64)),
            (Aggregator::Sum, true) => parse_quote!((i64, i64, Option<#expr_type>)),
            (Aggregator::Min, true) | (Aggregator::Max, true) => {
                parse_quote!((i64, std::collections::BTreeMap<#expr_type, usize>))
            }
            (Aggregator::Sum, false) => parse_quote!((i64, #expr_type)),
            (Aggregator::Min, false) | (Aggregator::Max, false) => {
                parse_quote!(std::collections::BTreeMap<#expr_type, usize>)
            }
            (Aggregator::Avg, true) => parse_quote!((i64, i64, Option<(i64, #expr_type)>)),
            (Aggregator::Avg, false) => parse_quote!((i64, #expr_type)),
            (Aggregator::CountDistinct, _) => unimplemented!(),
        }
    }

    fn memory_add_syn_expr(&self) -> syn::Expr {
        let input_nullable = self.incoming_expression.nullable();
        let expr_type = self.aggregate_type();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, _) => parse_quote!({
                arroyo_worker::operators::aggregating_window::count_add(current, bin_value)
            }),
            (Aggregator::Sum, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_sum_add::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Sum, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_sum_add::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Min, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_heap_add::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Min, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_heap_add::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Max, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_heap_add::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Max, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_heap_add::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Avg, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_average_add::<#expr_type>(
                    current, bin_value,
                )
            }),
            (Aggregator::Avg, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_average_add::<#expr_type>(
                    current, bin_value,
                )
            }),
            (Aggregator::CountDistinct, true) => todo!(),
            (Aggregator::CountDistinct, false) => todo!(),
        }
    }

    fn memory_remove_syn_expr(&self) -> syn::Expr {
        let input_nullable = self.incoming_expression.nullable();
        let expr_type = self.aggregate_type();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, true) | (Aggregator::Count, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::count_remove(current, bin_value)
            }),
            (Aggregator::Sum, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_sum_remove::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Sum, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_sum_remove::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Min, true) | (Aggregator::Max, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_heap_remove::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Min, false) | (Aggregator::Max, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_heap_remove::<#expr_type>(current, bin_value)
            }),
            (Aggregator::Avg, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_average_remove::<#expr_type>(
                    current, bin_value,
                )
            }),
            (Aggregator::Avg, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_average_remove::<#expr_type>(
                    current, bin_value,
                )
            }),
            (Aggregator::CountDistinct, true) => todo!(),
            (Aggregator::CountDistinct, false) => todo!(),
        }
    }

    fn return_type(&self) -> TypeDef {
        match self.aggregator {
            Aggregator::Count => TypeDef::DataType(DataType::Int64, false),
            Aggregator::Sum => self
                .aggregate_type_def()
                .with_nullity(self.incoming_expression.nullable()),
            Aggregator::Min => self.incoming_expression.return_type(),
            Aggregator::Max => self.incoming_expression.return_type(),
            Aggregator::Avg => match self.incoming_expression.return_type() {
                TypeDef::StructDef(_, _) => unreachable!(),
                TypeDef::DataType(data_type, nullable) => TypeDef::DataType(
                    avg_return_type(&data_type).expect("data fusion should've validated types"),
                    nullable,
                ),
            },
            Aggregator::CountDistinct => TypeDef::DataType(DataType::Int64, false),
        }
    }

    fn bin_aggregating_expression(&self) -> syn::Expr {
        let input_nullable = self.incoming_expression.nullable();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, _)
            | (Aggregator::Sum, _)
            | (Aggregator::Min, _)
            | (Aggregator::Max, _) => parse_quote!(arg.clone()),
            (Aggregator::Avg, true) => parse_quote!(match arg {
                Some((count, sum)) => Some((*sum as f64) / (*count as f64)),
                None => None,
            }),
            (Aggregator::Avg, false) => parse_quote!({ (arg.1 as f64) / (arg.0 as f64) }),
            (Aggregator::CountDistinct, true) => todo!(),
            (Aggregator::CountDistinct, false) => todo!(),
        }
    }

    fn to_aggregating_syn_expression(&self) -> syn::Expr {
        let input_nullable = self.incoming_expression.nullable();
        let expr_type = self.aggregate_type();
        match (&self.aggregator, input_nullable) {
            (Aggregator::Count, _) => {
                parse_quote!({ arroyo_worker::operators::aggregating_window::count_aggregate(arg) })
            }
            (Aggregator::Sum, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_sum_aggregate::<#expr_type>(arg)
            }),
            (Aggregator::Sum, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_sum_aggregate::<#expr_type>(arg)
            }),
            (Aggregator::Min, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_min_heap_aggregate::<#expr_type>(arg)
            }),
            (Aggregator::Min, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_max_heap_aggregate::<#expr_type>(arg)
            }),
            (Aggregator::Max, true) => parse_quote!({
                arroyo_worker::operators::aggregating_window::nullable_max_heap_aggregate::<#expr_type>(arg)
            }),
            (Aggregator::Max, false) => parse_quote!({
                arroyo_worker::operators::aggregating_window::non_nullable_max_heap_aggregate::<#expr_type>(arg)
            }),
            (Aggregator::Avg, true) => parse_quote!({
                match &arg.2 {
                    Some((count, sum)) => Some((*sum as f64) / (*count as f64)),
                    None => None,
                }
            }),
            (Aggregator::Avg, false) => parse_quote!({ (arg.1 as f64) / (arg.0 as f64) }),
            (Aggregator::CountDistinct, true) => unimplemented!(),
            (Aggregator::CountDistinct, false) => unimplemented!(),
        }
    }
}
