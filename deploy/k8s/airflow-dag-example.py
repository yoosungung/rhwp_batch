from airflow import DAG
from airflow.providers.cncf.kubernetes.operators.pod import KubernetesPodOperator
from datetime import datetime

with DAG(
    dag_id="rhwp_fill_example",
    start_date=datetime(2026, 1, 1),
    schedule=None,
    catchup=False,
) as dag:
    fill_task = KubernetesPodOperator(
        task_id="rhwp_fill",
        image="registry.local/rhwp-batch:0.1.0",
        arguments=[
            "fill",
            "--template=/templates/order.hwp",
            "--data-dir=/input",
            "--output-dir=/output",
            "--report=/output/_report.json",
            "--log-format=json",
        ],
        volumes=[
            # templates: ConfigMap or PVC containing template HWP files
            # input:     PVC containing JSON data files
            # output:    PVC for generated HWP output
        ],
        volume_mounts=[
            # configure volume mounts matching job-template.yaml
        ],
        namespace="default",
        get_logs=True,
        is_delete_operator_pod=True,
    )
