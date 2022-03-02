import QtQuick
import QtQuick.Window
import QtQuick.Controls

Item {
    id: page
    visible: true

    Column {
        anchors.centerIn: parent
        spacing: 10

        Text {
            text: "Current sessions:"
        }

        ListView {
            width: 300
            height: 200
            model: sessionModel.get_shares()

            // FIXME: show mapped drive name, show buttons for interaction
            // (closing session, re-map drive if it was umounted etc.)
            delegate: Text {
                text: dest_path
            }
        }

        Button {
            id: openSessionButton
            text: qsTr("New Session")
            onClicked: pageStateModel.startSessionDialog()
        }
    }
}
