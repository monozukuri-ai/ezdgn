# DGN test data

この directory の binary/CSV は、OSGeo/GDAL の次の固定 revision から取得した。

```text
repository: https://github.com/OSGeo/gdal
commit:     18e7cceb43a0dd58be474c9fdd5384baa3cde7c9
fetched:    2026-07-21
```

GDAL distribution の一般 license は MIT style であり、取得時点の全文を [`LICENSE.GDAL.txt`](LICENSE.GDAL.txt) に保存している。各 file は upstream の byte 列を変更せず、そのまま配置した。binary test data ごとの個別 license metadata は upstream にないため、これは GDAL distribution の一般条項に基づく整理であり、個別の法的判断ではない。

## Files

| Local file | Upstream path | Bytes | SHA-256 | 用途 |
| --- | --- | ---: | --- | --- |
| `v7/smalltest.dgn` | `autotest/ogr/data/dgn/smalltest.dgn` | 10,752 | `9d9faddb67216f9d56fc9a1027adc1a927c8c13529dfd3416b771b4dc4e9a284` | V7 2D 正常系 |
| `v7/seed_2d.dgn` | `ogr/ogrsf_frmts/dgn/data/seed_2d.dgn` | 9,216 | `dd8465f18569d9289809e9e0962115d365d0a56de021393952a5e7a0a20b527c` | V7 2D empty seed |
| `v7/seed_3d.dgn` | `ogr/ogrsf_frmts/dgn/data/seed_3d.dgn` | 2,048 | `97c2f00ee6ea96873b7d16e5e898b4850e3d35448299d7d9d37e7d1792b56896` | V7 3D 識別・座標分岐用 empty seed |
| `malformed/knot_oob.dgn` | `autotest/ogr/data/dgn/knot_oob.dgn` | 1,578 | `bd09d118a595f5afba833c7c61dad83cf02b01f1b224836b8493a164de90da4e` | V7 type 26 B-spline knot の malformed input |
| `v8/test_dgnv8.dgn` | `autotest/ogr/data/dgnv8/test_dgnv8.dgn` | 27,648 | `8f32f87ce4b16881aa64f5cb9f75c98851833f96fef37ca0ad31aa6bb18d1df0` | V8/CFB 識別と将来の V8 backend 用 |
| `v8/test_dgnv8_ref.csv` | `autotest/ogr/data/dgnv8/test_dgnv8_ref.csv` | 25,507 | `09f765e0aa8dc06bb1d0da4ed86f060dc0a839d480b3dc67e54841ecceec6433` | GDAL/ODA が出力した V8 sample の期待値 |
| `LICENSE.GDAL.txt` | `LICENSE.TXT` | 21,841 | `1dae3468e81d00da56e2936f74d33b8b3ad09d726437f19ce209a5dabea41f77` | upstream license |

`SHA256SUMS` は license file 自身を除く test artifact を検証する。上流 file への直接リンクは固定 commit を使う。

- [V7 data directory](https://github.com/OSGeo/gdal/tree/18e7cceb43a0dd58be474c9fdd5384baa3cde7c9/autotest/ogr/data/dgn)
- [V7 seed directory](https://github.com/OSGeo/gdal/tree/18e7cceb43a0dd58be474c9fdd5384baa3cde7c9/ogr/ogrsf_frmts/dgn/data)
- [V8 data directory](https://github.com/OSGeo/gdal/tree/18e7cceb43a0dd58be474c9fdd5384baa3cde7c9/autotest/ogr/data/dgnv8)
- [V7 upstream tests](https://github.com/OSGeo/gdal/blob/18e7cceb43a0dd58be474c9fdd5384baa3cde7c9/autotest/ogr/ogr_dgn.py)
- [V8 upstream tests](https://github.com/OSGeo/gdal/blob/18e7cceb43a0dd58be474c9fdd5384baa3cde7c9/autotest/ogr/ogr_dgnv8.py)

## Verified contents

ローカルの GDAL 3.8.4 V7 driver と独立した record scan で次を確認した。

### `smalltest.dgn`

- V7、2D、1 layer、graphic feature 4 件
- type 17 / level 1: text `Demo Text` at `(0.7365, 4.2198)`
- type 15 / level 2: ellipse（GDAL 表示近似 bbox `(0.328593, -0.0961066)` - `(9.68781, 9.26311)`）
- type 6 / level 2: filled shape
- type 3 / level 2: line
- raw scan は control/application element を含む 15 records
- units は `10 subunits/master * 1000 UOR/subunit = 10000 UOR/master`
- type 17 の offset-binary range は raw UOR `(7365, 37198)` -
  `(94083, 57198)`、master units では `(0.7365, 3.7198)` -
  `(9.4083, 5.7198)`
- type 6 は property word `0x0e00`、symbology word `0x5300`、attribute
  data は record-relative offset 78 から 16 bytes
- type 17 はfont ID 3、justification 7、raw text `44 65 6d 6f 20 54 65 78 74`
  を持ち、originはraw UOR `(7365, 42198)`、master `(0.7365, 4.2198)`
- type 15 はcenterがraw UOR約`(50082, 45835)`、master
  `(5.0082, 4.5835)`、2軸はraw UOR約`46796.06584`、master約`4.679606584`
- type 6 の符号化頂点列は`(45355,33170)`, `(43832,26517)`,
  `(49441,25235)`, `(48320,33331)`, `(45355,33170)`
- type 3 の端点はraw UOR `(25562,57218)` - `(25242,60709)`、master
  `(2.5562,5.7218)` - `(2.5242,6.0709)`

### seed files

- `seed_2d.dgn`: V7 2D、graphic feature 0、12 records、master/sub unit 名 `ft` / `tf`、120 UOR/master
- `seed_3d.dgn`: V7 3D、graphic feature 0、3 records、master/sub unit 名 `m` / `mm`、1000 UOR/master
- `seed_2d.dgn` のVAX D-Float global originはraw UORで
  `(-249879416, -669487710, 0)`、master unitsで
  `(-2082328.4666666666, -5579064.25, 0)`

### malformed file

`knot_oob.dgn` は TCB の後に type 26 B-spline knot record を一つ持ち、A-bitが
clearなのに未使用のattribute indexが`0xffff`になっているため、正常fixtureとして
扱わない。raw scannerは40 byte recordをそのまま保持する。Phase 4 semantic readerは
A-bitがclearのとき未使用indexを参照せず、boundedな32 bit knot値1個（0）を読む。
crash、out-of-bounds、無限loopがないことを回帰テストにする。

### V8 sample

- CFB signature: `d0 cf 11 e0 a1 b1 1a e1`
- CFB sector size 512、mini sector size 64
- metadata の creating application: `Teigha DGN 4.02.2.0`
- storage/stream には `Dgn~H`、`Dgn~S`、`Dgn-Md/#000000/...` 等がある
- GDAL の V8 test では model/layer 名は `my_model`
- reference CSV は 34 features（2D 19、3D 15）を持つ
- type は 2, 3, 4, 6, 11, 12, 14, 15, 16, 17, 22, 27, 35, 36 を含む
- geometry は point、line string、polygon、multi-point、complex/curve geometry を含み、Unicode text `myTéxt` も含む
- Phase 6 の native CFB inspector では CFB version 3、root を除く storage 9、stream 15、計24 entry、model storage `/Dgn-Md/#000000` と判定する

ローカル GDAL build には ODA dependency がないため、V8 sample が V7 driver では開けないことも確認した。CSV は将来 licensed backend または外部変換の fidelity を検証するときの oracle であり、CFB inspector や V7 parser の期待値ではない。

## Integrity check

repository root から実行する。

```bash
sha256sum -c tests/data/dgn/SHA256SUMS
```
