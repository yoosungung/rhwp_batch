"""
rhwp-batch Airflow DAG 예시 (KubernetesPodOperator)

사용 패턴:
  - fill: JSON 데이터 N건을 양식 HWP에 채워 HWP N개 생성 (배치 모드)
  - to-json: HWP/HWPX 파일을 RAG-friendly JSON으로 변환

볼륨은 클러스터 환경에 맞게 조정하십시오 (PVC, ConfigMap, CSI 드라이버 등).
"""
from __future__ import annotations

from datetime import datetime

from airflow import DAG
from airflow.providers.cncf.kubernetes.operators.pod import KubernetesPodOperator
from kubernetes.client import models as k8s

IMAGE = "registry.local/rhwp-batch:0.1.0"

# ── 볼륨 정의 ─────────────────────────────────────────────────────────────────
volumes = [
    k8s.V1Volume(
        name="templates",
        config_map=k8s.V1ConfigMapVolumeSource(name="rhwp-templates"),
    ),
    k8s.V1Volume(
        name="input",
        persistent_volume_claim=k8s.V1PersistentVolumeClaimVolumeSource(
            claim_name="rhwp-input"
        ),
    ),
    k8s.V1Volume(
        name="output",
        persistent_volume_claim=k8s.V1PersistentVolumeClaimVolumeSource(
            claim_name="rhwp-output"
        ),
    ),
]

volume_mounts = [
    k8s.V1VolumeMount(name="templates", mount_path="/templates", read_only=True),
    k8s.V1VolumeMount(name="input", mount_path="/input", read_only=True),
    k8s.V1VolumeMount(name="output", mount_path="/output"),
]

# ── fill DAG ─────────────────────────────────────────────────────────────────
with DAG(
    dag_id="rhwp_fill_batch",
    start_date=datetime(2026, 1, 1),
    schedule=None,
    catchup=False,
    tags=["rhwp", "hwp", "batch"],
) as fill_dag:
    fill_task = KubernetesPodOperator(
        task_id="rhwp_fill",
        image=IMAGE,
        arguments=[
            "fill",
            "--template=/templates/order.hwp",
            "--data-dir=/input",
            "--output-dir=/output",
            "--report=/output/_report.json",
            "--on-error=continue",
            "--log-format=json",
        ],
        volumes=volumes,
        volume_mounts=volume_mounts,
        namespace="default",
        get_logs=True,
        is_delete_operator_pod=True,
        # exit code 4 = 부분 실패 (D10) — Airflow는 0 이외를 실패로 처리
        # on_finish_action="keep_pod" 로 로그 보존 가능
    )

# ── to-json DAG ───────────────────────────────────────────────────────────────
with DAG(
    dag_id="rhwp_to_json",
    start_date=datetime(2026, 1, 1),
    schedule=None,
    catchup=False,
    tags=["rhwp", "hwp", "rag", "conversion"],
) as tojson_dag:
    tojson_task = KubernetesPodOperator(
        task_id="rhwp_to_json",
        image=IMAGE,
        arguments=[
            "to-json",
            "--input-dir=/input",
            "--output-dir=/output",
            "--image-mode=extract",
            "--log-format=json",
        ],
        volumes=[v for v in volumes if v.name != "templates"],
        volume_mounts=[m for m in volume_mounts if m.name != "templates"],
        namespace="default",
        get_logs=True,
        is_delete_operator_pod=True,
    )
