# @author ixxxxoooo
"""生成 PDF OCR 测试夹具(依赖 Pillow,macOS 中文字体)。运行: python3 make_fixtures.py"""
from PIL import Image, ImageDraw, ImageFont

FONT = "/System/Library/Fonts/Hiragino Sans GB.ttc"


def font(size):
    return ImageFont.truetype(FONT, size)


def page(draw_fn, size=(1240, 1754)):
    img = Image.new("L", size, 255)
    draw_fn(ImageDraw.Draw(img))
    return img


def body(d):
    d.text((120, 140), "年度预算复盘报告", font=font(64), fill=0)
    y = 300
    for line in [
        "本季度市场部的差旅费用同比下降了百分之十二,",
        "设备采购集中在第三周完成,供应商为星河科技。",
        "下一阶段的重点是压缩推广成本并提升转化率。",
    ]:
        d.text((120, y), line, font=font(38), fill=0)
        y += 80


def table(d):
    d.text((120, 120), "费用明细", font=font(56), fill=0)
    rows = [("项目", "金额", "负责人"), ("差旅", "1200", "张三"), ("设备", "8800", "李四"), ("推广", "5600", "王五")]
    x0, y0, cw, rh = 120, 260, 320, 90
    for r, row in enumerate(rows):
        for c, cell in enumerate(row):
            d.text((x0 + c * cw + 20, y0 + r * rh + 20), cell, font=font(40), fill=0)
    for r in range(len(rows) + 1):
        d.line((x0, y0 + r * rh, x0 + 3 * cw, y0 + r * rh), fill=0, width=2)
    for c in range(4):
        d.line((x0 + c * cw, y0, x0 + c * cw, y0 + len(rows) * rh), fill=0, width=2)


page(body).save("scan_cn.pdf", "PDF", resolution=150)
page(table).save("scan_table.pdf", "PDF", resolution=150)
# 旋转页:把版面逆时针旋转后存成 PDF,再声明 /Rotate 90 让阅读器转正
rot = page(body).rotate(90, expand=True)
rot.save("scan_rotated.pdf", "PDF", resolution=150)
data = open("scan_rotated.pdf", "rb").read()
data = data.replace(b"/Type /Page\n", b"/Type /Page\n/Rotate 90\n", 1) if b"/Type /Page\n" in data else data
open("scan_rotated.pdf", "wb").write(data)

# 带文本层的 PDF(手写最小结构,英文)
content = b"BT /F1 20 Tf 72 720 Td (Quarterly Review) Tj 0 -40 Td (Travel cost fell twelve percent year over year.) Tj ET"
objs = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
    b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
]
out = bytearray(b"%PDF-1.4\n")
offs = []
for i, o in enumerate(objs, 1):
    offs.append(len(out))
    out += b"%d 0 obj\n" % i + o + b"\nendobj\n"
xref = len(out)
out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
for o in offs:
    out += b"%010d 00000 n \n" % o
out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
open("text_en.pdf", "wb").write(bytes(out))


# ---- 图片 OCR 夹具(图文关联测试用):白底黑字的小图 ----
def text_png(name, lines, size=(900, 260)):
    img = Image.new("RGB", size, (255, 255, 255))
    d = ImageDraw.Draw(img)
    y = 40
    for line in lines:
        d.text((40, y), line, font=font(56), fill=(0, 0, 0))
        y += 90
    img.save(name, "PNG")


text_png("ocr_budget.png", ["设备采购预算", "总计八万八千元"])
text_png("ocr_route.png", ["星河科技供应商", "交付周期三周"])
