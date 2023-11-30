use crate::translation::{error::Error, helpers::Env};
use ndc_sdk::models;
use query_engine_metadata::metadata;
use query_engine_sql::sql;

/// Maps a binary comparison operator to their appropriate PostgreSQL name and arguments type.
pub fn translate_comparison(
    env: &Env,
    left_type: &metadata::ScalarType,
    operator: &models::BinaryComparisonOperator,
) -> Result<(sql::ast::Function, metadata::ScalarType), Error> {
    match operator {
        models::BinaryComparisonOperator::Equal => Ok((sql::ast::equals(), left_type.clone())),

        models::BinaryComparisonOperator::Other { name } => {
            let op = env.lookup_comparison(left_type, name)?;

            Ok((
                sql::ast::Function {
                    function_name: op.operator_name.clone(),
                    is_infix: op.is_infix,
                },
                op.argument_type.clone(),
            ))
        }
    }
}
