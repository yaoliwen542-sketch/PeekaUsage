# 截图后处理：从 DSF 1.25 原图裁剪左上角目标 CSS 区域并还原到 1x
# 用法: python post.py <原图> <输出> <目标CSS宽> <目标CSS高>
import sys
from PIL import Image

src, dst, w, h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
im = Image.open(src)
im = im.crop((0, 0, round(w * 1.25), round(h * 1.25)))
im = im.resize((w, h), Image.LANCZOS)
im.save(dst)
