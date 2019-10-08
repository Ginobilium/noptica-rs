import asyncio
from PyQt5 import QtCore, QtGui, QtWidgets
import pyqtgraph
import quamash
import atexit

async def task(mw):
    widget = pyqtgraph.PlotWidget()
    mw.setCentralWidget(widget)
    mw.show()

    process = await asyncio.create_subprocess_shell("cargo run --release",
        stdout=asyncio.subprocess.PIPE)
    data = []
    while True:
        line = await process.stdout.readline()
        if not line:
            print("input process died")
            break
        number = int(line.decode().strip())
        data.append(number)
        widget.clear()
        widget.plot(data)



def main():
    app = QtWidgets.QApplication(["Plot"])
    loop = quamash.QEventLoop(app)
    asyncio.set_event_loop(loop)

    mw = QtWidgets.QMainWindow()
    mw.showMaximized()

    atexit.register(loop.close)
    asyncio.ensure_future(task(mw))
    loop.run_forever()

    
main()
