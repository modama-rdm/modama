import QtQuick
import QtQuick.Window
import QtQuick.Controls

Item {
    id: page

    visible: true

    Column {
        anchors.centerIn: parent
        spacing: 10

        TextField {
            id: usernameInput
            text: loginModel.username
            placeholderText: qsTr("Enter username")
            onTextEdited: loginModel.username = text
            KeyNavigation.tab: passwordInput
            enabled: !loginModel.busy
        }
        TextField {
            id: passwordInput
            text: loginModel.password
            placeholderText: qsTr("Enter password")
            echoMode: TextInput.Password
            onTextEdited: loginModel.password = text
            KeyNavigation.tab: loginButton
            Keys.onReturnPressed: loginModel.doLogin()
            enabled: !loginModel.busy
            focus: true
        }
        Button {
            id: loginButton
            text: qsTr("Login")
            KeyNavigation.tab: usernameInput
            onClicked: loginModel.doLogin()
            anchors.right: parent.right
            enabled: !loginModel.busy
        }
    }
    BusyIndicator {
        visible: loginModel.busy
        anchors.centerIn: parent
    }
}