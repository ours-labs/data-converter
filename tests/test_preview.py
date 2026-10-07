import json
import unittest
from pgcsv.remote import preview_json, preview_query, PREVIEW_MAX_BYTES, PREVIEW_MAX_COLUMNS


class PreviewTests(unittest.TestCase):
    def test_unicode_null_and_truncated_text(self):
        rows = [[None, '日本語…', '1024']]
        result = json.loads(preview_json(['empty', 'text', 'number'], rows))
        self.assertEqual(result['rows'], rows)
        self.assertEqual(result['cell_character_limit'], 1024)

    def test_oversize_response_and_column_count_rejected(self):
        with self.assertRaisesRegex(ValueError, 'size limit'):
            preview_json(['text'], [['x' * PREVIEW_MAX_BYTES]])
        # This check occurs before any SQL object or cursor is constructed.
        with self.assertRaisesRegex(ValueError, 'too many columns'):
            preview_query(None, None, None, ['x'] * (PREVIEW_MAX_COLUMNS + 1))


if __name__ == '__main__':
    unittest.main()
