# Sitemap

| URL                                                              | Method | Description                      |
| ---------------------------------------------------------------- | ------ | -------------------------------- |
| `/`                                                              | GET    | Home page listing libraries      |
| `/api/import/candidates`                                         | GET    | External title candidates        |
| `/api/library/{id}/publication/{publication}/contents/add-range` | POST   | Add a numbered range of works    |
| `/api/library/{id}/publication/{publication}/contents/add`       | POST   | Add a work to a publication      |
| `/api/library/{id}/publication/{publication}/contents/remove`    | POST   | Remove a work from a publication |
| `/api/library/{id}/publication/{publication}/contributor/add`    | POST   | Add a publication contributor    |
| `/api/library/{id}/publication/{publication}/contributor/remove` | POST   | Remove a publication contributor |
| `/api/library/{id}/suggest/{field}`                              | GET    | Field value suggestions          |
| `/api/library/{id}/work/{work}/contributor/add`                  | POST   | Add a work contributor           |
| `/api/library/{id}/work/{work}/contributor/remove`               | POST   | Remove a work contributor        |
| `/api/review/{kind}/{entity}/cancel`                             | POST   | Cancel enrichment                |
| `/api/review/{kind}/{entity}/discard`                            | POST   | Confirm, then discard            |
| `/api/review/{kind}/{entity}/retry`                              | POST   | Retry enrichment                 |
| `/api/review`                                                    | GET    | Review queue widget              |
| `/api/search`                                                    | GET    | Search suggestions               |
| `/api/stage/{staged}/discard`                                    | POST   | Discard a staged file            |
| `/api/stage/{staged}`                                            | GET    | Preview a staged file            |
| `/api/stage`                                                     | POST   | Stage a file or URL              |
| `/assets/{name}`                                                 | GET    | Static assets                    |
| `/library/{id}/delete`                                           | POST   | Delete a library                 |
| `/library/{id}/edit`                                             | GET    | Edit or delete a library         |
| `/library/{id}/edit`                                             | POST   | Save a library                   |
| `/library/{id}/import`                                           | GET    | Import a publication             |
| `/library/{id}/import`                                           | POST   | Create a draft                   |
| `/library/{id}/people/{role}`                                    | GET    | People with one role             |
| `/library/{id}/people`                                           | GET    | People in a library              |
| `/library/{id}/person/{person}/commit`                           | POST   | Save and commit a person         |
| `/library/{id}/person/{person}/delete`                           | POST   | Delete a person                  |
| `/library/{id}/person/{person}/edit`                             | GET    | Edit or delete a person          |
| `/library/{id}/person/{person}/edit`                             | POST   | Save a person                    |
| `/library/{id}/person/{person}/profile`                          | GET    | Profile picture                  |
| `/library/{id}/person/{person}`                                  | GET    | Person details                   |
| `/library/{id}/publication/{publication}/commit`                 | POST   | Save and commit a publication    |
| `/library/{id}/publication/{publication}/cover`                  | GET    | Cover image                      |
| `/library/{id}/publication/{publication}/delete`                 | POST   | Delete a publication             |
| `/library/{id}/publication/{publication}/edit`                   | GET    | Edit or delete a publication     |
| `/library/{id}/publication/{publication}/edit`                   | POST   | Save a publication               |
| `/library/{id}/publication/{publication}/holding/{holding}`      | GET    | Digital holding file             |
| `/library/{id}/publication/{publication}`                        | GET    | Publication details              |
| `/library/{id}/tag/{tag}`                                        | GET    | Entries with one tag             |
| `/library/{id}/tags`                                             | GET    | Tags in a library                |
| `/library/{id}/work/{work}/commit`                               | POST   | Save and commit a work           |
| `/library/{id}/work/{work}/delete`                               | POST   | Delete a work                    |
| `/library/{id}/work/{work}/edit`                                 | GET    | Edit or delete a work            |
| `/library/{id}/work/{work}/edit`                                 | POST   | Save a work                      |
| `/library/{id}/work/{work}`                                      | GET    | Work details                     |
| `/library/{id}`                                                  | GET    | Library contents                 |
| `/library`                                                       | POST   | Create a library                 |
| `/login`                                                         | GET    | Log in                           |
| `/login`                                                         | POST   | Submit the password              |
| `/logout`                                                        | POST   | Log out                          |
| `/review`                                                        | GET    | Review queue                     |
| `/search`                                                        | GET    | Search results                   |
| `/settings/audit`                                                | GET    | Audit log                        |
| `/settings/password`                                             | GET    | Change the password              |
| `/settings/password`                                             | POST   | Save the new password            |
| `/settings`                                                      | GET    | Settings                         |
