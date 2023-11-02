#[cfg(test)]
/// create a fresh db then run a query against it
mod basic {
    use super::super::common;
    use tests_common::deployment::{clean_up_deployment, create_fresh_deployment};
    use tests_common::request::run_query;

    #[ignore = "for some reason this takes a long time"]
    #[tokio::test]
    async fn select_by_pk() {
        let deployment =
            create_fresh_deployment(common::CONNECTION_STRING, common::CHINOOK_DEPLOYMENT_PATH)
                .await;

        let result = run_query(
            tests_common::router::create_router_from_deployment(&deployment.deployment_path).await,
            "select_by_pk",
        )
        .await;

        clean_up_deployment(deployment).await;
        insta::assert_json_snapshot!(result)
    }
}