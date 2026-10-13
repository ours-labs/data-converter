# Error reference

Applies to 0.3.0-preview.1 and later unless stated otherwise.

Codes identify a failure category, not a unique incident. Details stay on your PC and are not sent to GitHub. A generic code does not establish the root cause.

## ODC-SSH-001

Could not establish the SSH connection.

Check the host, network, SSH authentication and host key.

## ODC-DB-001

The database operation failed.

Check SSH forwarding permission, database endpoint, availability, permissions and TLS settings.

## ODC-DB-002

Database authentication failed.

Check the database user and credentials. These differ from SSH credentials.

## ODC-CONFIG-001

Check the input or connection settings.

Check required fields, ports, date range and settings format.

## ODC-SAVE-001

Could not complete the save operation.

Check free space, destination permissions, existing files and the input CSV. Do not use incomplete output as a completed export.

## ODC-CANCEL-001

The operation was cancelled.

Review the settings before retrying. An incomplete folder may remain.

## ODC-TIMEOUT-001

The operation timed out.

Check connectivity and database load, then retry with a smaller selection.

## ODC-HISTORY-001

Could not process history or saved settings.

Check access to the settings storage. You can enter connection details manually.

## ODC-UNKNOWN-001

Could not complete the operation.

Read the details before retrying. The cause has not been identified.
