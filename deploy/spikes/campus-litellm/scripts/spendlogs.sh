#!/bin/sh
# Prints LiteLLM_SpendLogs as CSV (no prompt bodies, only their lengths) for driver.py report --spendlogs -
exec kubectl -n campus-litellm-spike exec deploy/postgres -- psql -U litellm -d litellm -At -c "COPY (SELECT request_id, api_key, spend, status, model_group, \"user\", team_id, end_user, length(messages::text) AS messages_len, length(response::text) AS response_len, length(proxy_server_request::text) AS psr_len FROM \"LiteLLM_SpendLogs\") TO STDOUT WITH CSV HEADER"
