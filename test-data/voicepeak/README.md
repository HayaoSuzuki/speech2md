# VOICEPEAK話者分離テスト原稿

このディレクトリには、VOICEPEAK 6ナレーターで話者分離用の音声を作るためのCSVファイルを収録しています。
原稿はすべてこのプロジェクト用に作成したものです。

## テストケース

| CSV | 予期話者数 | 用途 |
| --- | ---: | --- |
| `balanced-4speakers.csv` | 4 | 4話者が同じ文章を同じ回数読んだ基本ケース |
| `male-3speakers.csv` | 3 | 男性音声だけの分離 |
| `pair-male1-male3.csv` | 2 | 男性1と男性3の識別 |
| `pair-male1-male2.csv` | 2 | 男性1と男性2の識別 |
| `pair-male2-male3.csv` | 2 | 男性2と男性3の識別 |
| `imbalanced-4speakers.csv` | 4 | 男性3だけが1発話の少数話者条件 |
| `single-male1.csv` | 1 | 男性1の単独基準 |
| `single-male3.csv` | 1 | 男性3の単独基準 |
| `single-male2.csv` | 1 | 男性2の単独基準 |
| `single-female1.csv` | 1 | 女性1の単独基準 |

均等ケースでは各話者に同じ文章を割り当てています。
これにより、発話内容の違いではなく音声特徴の違いを中心に比較できます。

## VOICEPEAKへの読み込み

1. VOICEPEAKを起動します。
2. 「ファイル」からテキストファイルの読み込みを選びます。
3. 読み込み形式としてCSVを選び、対象のCSVファイルを指定します。
4. 第1列が「男性1」「男性2」「男性3」「女性1」のナレーター指定として認識されたことを確認します。
5. 速度、ピッチ、感情、ポーズを変更せず、すべてのケースを同じ設定で書き出します。

ナレーター名が本文として読み込まれた場合は、VOICEPEAKの表示名とCSVの第1列が一致していません。
その場合は、CSVの第1列を現在の表示名に置換します。

操作の詳細はVOICEPEAK公式マニュアルの[テキストファイルを読み込む](https://www.ah-soft.com/voice/manual/03_useage.html#3-8)と[CSV形式のテキストファイルを読み込む](https://www.ah-soft.com/voice/manual/03_useage.html#3-11)を参照してください。

## 音声の書き出し

各プロジェクトをWAV形式で書き出し、CSVと同じベース名で`samples/voicepeak/`に保存します。
例えば、`balanced-4speakers.csv`の音声は`samples/voicepeak/balanced-4speakers.wav`です。
VOICEPEAKのプロジェクトを残す場合は、同じディレクトリに同名のVPPファイルを保存できます。
`samples/voicepeak/`はGitの追跡対象外です。

SRT形式は時刻を指定できますが、このテストで必要な行ごとのナレーター指定にはCSVのほうが適しています。
SSML形式は読み方やポーズを制御したいテストで利用できます。
代替形式の仕様はVOICEPEAK公式マニュアルの[SRT形式のテキストファイルを読み込む](https://www.ah-soft.com/voice/manual/03_useage.html#3-9)と[SSML形式のテキストファイルを読み込む](https://www.ah-soft.com/voice/manual/03_useage.html#3-12)を参照してください。

## 推奨する評価順序

1. 4本の単独基準で、話者数がそれぞれ1になることを確認します。
2. 3本の男性ペアで、識別しにくい組み合わせを特定します。
3. `male-3speakers.wav`で、男性3話者を同時に分離できるか確認します。
4. `balanced-4speakers.wav`で、4話者の均等条件を確認します。
5. `imbalanced-4speakers.wav`で、発話の少ない男性3を検出できるか確認します。

CSVの構造は次のテストで検証できます。

```console
cargo test -p yasumaro-runtime --test voicepeak_sources --locked
```
