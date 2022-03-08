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

        ListModel {
            id: someModel
            ListElement {
                dest_path: "stuff1"
            }
            ListElement {
                dest_path: "stuff2"
            }
            ListElement {
                dest_path: "stuff3"
            }
        }

        ListView {
            width: 300
            height: 200
            //model: someModel
            model: sessionModel.get_shares()

            // FIXME: show mapped drive name, show buttons for interaction
            // (closing session, re-map drive if it was umounted etc.)
            delegate: Text {
                text: "stuff: " + dest_path
            }
        }

        ComboBox {
            id: comboBox
            // flat: false
            editable: false
            width: 300
            model: sessionModel.get_shares()
            textRole: "uid"
        }

        Button {
            id: openSessionButton
            text: qsTr("New Session")
            onClicked: pageStateModel.startSessionDialog()
        }
    }
}
