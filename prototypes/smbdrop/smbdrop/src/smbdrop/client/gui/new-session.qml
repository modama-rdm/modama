import QtQuick
import QtQuick.Window
import QtQuick.Controls

Item {
    id: page
    visible: true

    Column {
        anchors.centerIn: parent
        spacing: 10

        ComboBox {
            id: comboBox
            // flat: false
            editable: false
            width: 300
            model: projectListModel
            textRole: "display"
            enabled: !sessionModel.busy
            onActivated: sessionModel.projectName = currentValue
        }

        TextField {
            id: sessionNameInput
            width: 300
            text: sessionModel.sessionName
            placeholderText: qsTr("Session name")
            onTextEdited: sessionModel.sessionName = text
            KeyNavigation.tab: openSessionButton
            enabled: !sessionModel.busy
        }

        Button {
            id: openSessionButton
            anchors.right: parent.right
            text: qsTr("Start session")
            onClicked: sessionModel.startSession()
            enabled: !sessionModel.busy
        }
    }

    BusyIndicator {
        visible: sessionModel.busy
        anchors.centerIn: parent
    }
}
