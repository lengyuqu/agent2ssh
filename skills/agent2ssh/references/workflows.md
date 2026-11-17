# Workflow patterns

Part of the agent2ssh skill. `SKILL.md` has the overview and the rules that always
apply; this file is the detail for one area.

## Workflow Patterns

### Diagnose a server

```
1. ssh_ping → confirm reachable
2. ssh_exec "df -h && free -m && ps aux --sort=-%cpu | head" → overview
3. ssh_exec "journalctl -u myapp --since '5 min ago'" → recent logs
4. ssh_sftp_ls host /var/log/myapp → check log files
```

### Deploy an artifact

```
1. ssh_sftp_mkdir host /opt/releases/<version>
2. ssh_sftp_upload host ./dist.tar.gz /opt/releases/<version>/dist.tar.gz
3. ssh_exec host "tar -xzf /opt/releases/<version>/dist.tar.gz -C /opt/app" (medium risk)
4. ssh_exec host "systemctl restart myapp" --force true (high risk — confirm first)
```

### Run a migration on multiple DB replicas

```
1. ssh_ping → verify all reachable
2. ssh_exec_multi hosts command:"pg_dump -Fc mydb > /tmp/backup.dump" timeout_secs:300
3. Confirm backups exist with ssh_exec_multi "ls -lh /tmp/backup.dump"
4. ssh_exec_multi command:"psql mydb < migration.sql" force:true
```
