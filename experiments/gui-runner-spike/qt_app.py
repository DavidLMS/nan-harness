import sys
from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout, QLineEdit, QPushButton

app = QApplication(sys.argv)
app.setApplicationName("NaN GUI Spike Qt")
window = QWidget()
window.setWindowTitle("NaN GUI Spike Qt")
layout = QVBoxLayout(window)
message = QLineEdit()
message.setAccessibleName("Message")
result = QLineEdit()
result.setReadOnly(True)
result.setAccessibleName("Result")
send = QPushButton("Send")
send.clicked.connect(lambda: result.setText("Received: " + message.text()))
for widget in (message, send, result):
    layout.addWidget(widget)
window.resize(600, 240)
window.show()
sys.exit(app.exec())
