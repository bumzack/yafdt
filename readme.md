# 


## requests

```
curl -X POST http://localhost:4023/api/duplicates -H "Content-Type: application/json" -d '
 {
  "root_folder": "/Users/bumzack/stoff/coding/",
  "target_folder": "/Users/bumzack/stoff/tmp_duplicates",
  "skip_folders": [
    ".git",
    "node_modules",
    "target"
  ],
  "skip_filenames": [],
  "min_file_size": 1000000,
  "consider_extensions": [
    "mov",
    "zip",
    "tar",
    "png",
    "jpg",
    "jpeg"
  ]
}
'  | jq > duplicates.json
```