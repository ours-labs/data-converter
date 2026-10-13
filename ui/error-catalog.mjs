export default {
  "ODC-SSH-001": {
    "ja": {
      "summary": "SSH接続を確認できませんでした。",
      "action": "接続先、ネットワーク、SSH認証とホスト鍵を確認してください。"
    },
    "en": {
      "summary": "Could not establish the SSH connection.",
      "action": "Check the host, network, SSH authentication and host key."
    }
  },
  "ODC-DB-001": {
    "ja": {
      "summary": "データベースの処理に失敗しました。",
      "action": "SSH転送の許可、DBの接続先・稼働状態・権限・TLS設定を確認してください。"
    },
    "en": {
      "summary": "The database operation failed.",
      "action": "Check SSH forwarding permission, database endpoint, availability, permissions and TLS settings."
    }
  },
  "ODC-DB-002": {
    "ja": {
      "summary": "データベースの認証に失敗しました。",
      "action": "DBユーザーと認証情報を確認してください。SSHの認証情報とは異なります。"
    },
    "en": {
      "summary": "Database authentication failed.",
      "action": "Check the database user and credentials. These differ from SSH credentials."
    }
  },
  "ODC-CONFIG-001": {
    "ja": {
      "summary": "入力または接続設定を確認してください。",
      "action": "必須項目、ポート、期間、設定形式を確認してください。"
    },
    "en": {
      "summary": "Check the input or connection settings.",
      "action": "Check required fields, ports, date range and settings format."
    }
  },
  "ODC-SAVE-001": {
    "ja": {
      "summary": "保存処理を完了できませんでした。",
      "action": "保存先の空き容量・権限・既存ファイルと入力CSVを確認してください。未完成の出力は完成品として扱わないでください。"
    },
    "en": {
      "summary": "Could not complete the save operation.",
      "action": "Check free space, destination permissions, existing files and the input CSV. Do not use incomplete output as a completed export."
    }
  },
  "ODC-CANCEL-001": {
    "ja": {
      "summary": "処理を中止しました。",
      "action": "入力を確認して再実行できます。未完成フォルダーは保持される場合があります。"
    },
    "en": {
      "summary": "The operation was cancelled.",
      "action": "Review the settings before retrying. An incomplete folder may remain."
    }
  },
  "ODC-TIMEOUT-001": {
    "ja": {
      "summary": "処理がタイムアウトしました。",
      "action": "通信状態とDBの負荷を確認し、対象範囲を狭めて再試行してください。"
    },
    "en": {
      "summary": "The operation timed out.",
      "action": "Check connectivity and database load, then retry with a smaller selection."
    }
  },
  "ODC-HISTORY-001": {
    "ja": {
      "summary": "履歴または設定を処理できませんでした。",
      "action": "保存先へのアクセスを確認してください。手入力で接続できます。"
    },
    "en": {
      "summary": "Could not process history or saved settings.",
      "action": "Check access to the settings storage. You can enter connection details manually."
    }
  },
  "ODC-UNKNOWN-001": {
    "ja": {
      "summary": "操作を完了できませんでした。",
      "action": "詳細を確認して再試行してください。原因を特定できていません。"
    },
    "en": {
      "summary": "Could not complete the operation.",
      "action": "Read the details before retrying. The cause has not been identified."
    }
  }
};
