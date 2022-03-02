from typing import Dict, List, Optional
import signal
import time
import sys
import os

import requests
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlApplicationEngine, qmlRegisterType
from PySide6.QtQuickControls2 import QQuickStyle
from PySide6.QtCore import (
    Qt, QObject, Signal, Slot, Property, QRunnable, QThreadPool,
    QAbstractListModel,
)

from smbdrop.client import ShareClient


PERMISSIONS_API_BASE_URL = "http://172.28.0.2:5050"
SMBDROP_API_BASE_URL = "http://127.0.0.1:5050"


class DoLogin(QRunnable):
    def __init__(self, model: "LoginModel") -> None:
        super().__init__()
        self.model = model

    @Slot()
    def run(self) -> None:
        self.model._set_busy(True)
        try:
            time.sleep(1)  # simulate network...
            print("success!")
            self.model._set_logged_in(True)
        finally:
            self.model._set_busy(False)


class DoListProjects(QRunnable):
    def __init__(self, model: "ProjectListModel", login_model: "LoginModel") -> None:
        super().__init__()
        self.model = model
        self.login_model = login_model

    @Slot()
    def run(self) -> None:
        url = f"{PERMISSIONS_API_BASE_URL}/project/"
        resp = requests.get(url).json()
        print(f"got list of projects: {resp['projects']}")
        self.model._set_projects([
            project["name"]
            for project in resp["projects"]
            if self.login_model.username in project["members"]
        ])


class DoListSessions(QRunnable):
    def __init__(self, model: "SessionModel", login_model: "LoginModel") -> None:
        super().__init__()
        self.model = model
        self.login_model = login_model

    @Slot()
    def run(self) -> None:
        # FIXME: error handling!
        try:
            c = ShareClient(SMBDROP_API_BASE_URL)
            shares = c.list()
            # TODO: server should filter by the UID we are logged in with
            share_list = list(shares.values())
            print(f"got list of shares: {share_list}")
            self.model.get_shares().replace_data(share_list)
        finally:
            self.model._set_busy(False)


class DoCreateSession(QRunnable):
    def __init__(self, model: "SessionModel", login_model: "LoginModel", project_name: str, session_name: str) -> None:
        super().__init__()
        self.model = model
        self.login_model = login_model
        assert project_name is not None
        assert session_name is not None
        self.project_name = project_name
        self.session_name = session_name

    @Slot()
    def run(self) -> None:
        # FIXME: error handling!
        try:
            project_name = self.project_name
            session_name = self.session_name

            # first, create the "dataset" via the "permissions API":
            url = f"{PERMISSIONS_API_BASE_URL}/project/{project_name}/dataset/"
            resp = requests.post(url, json={
                "name": session_name,
                "owner": self.login_model.username,
            })
            if "message" in resp.json():
                print(resp.json()["message"])
            resp.raise_for_status()

            c = ShareClient(SMBDROP_API_BASE_URL)
            share = c.create(
                # FIXME: `dest_path` should be set by the high-level API - or more clearly,
                # it should be retrieved from the "permissions API", via the high-level API
                dest_path=f"/data/adhoc/{project_name}/{session_name}/",
                # FIXME: uid/gid should be set by the high level API from the
                # credentials we log in with
                uid=1000,
                gid=1000,
            )
            shares = c.list()
            self.model._set_shares(list(shares.values()))
            self.model._set_session_share_id(share["share_id"])
        finally:
            self.model._set_busy(False)
            self.model.sessionStart.emit()


class LoginModel(QObject):
    # TODO: add failure case
    def __init__(self) -> None:
        super().__init__()
        self._username: str = ""
        self._password: str = ""
        self._logged_in: bool = False
        self._busy: bool = False
        self.threadpool = QThreadPool()

    usernameChanged = Signal(str)

    def get_username(self) -> str:
        return self._username

    def set_username(self, new_name: str) -> None:
        print(f"set_username({new_name})")
        if new_name != self._username:
            self._username = new_name
            self.usernameChanged.emit(new_name)

    username = Property(str, get_username, set_username, notify=usernameChanged)

    passwordChanged = Signal(str)

    def get_password(self) -> str:
        return self._password

    def set_password(self, new_password: str) -> None:
        if new_password != self._password:
            self._password = new_password
            self.passwordChanged.emit(new_password)

    password = Property(str, get_password, set_password, notify=passwordChanged)

    busyChanged = Signal(bool)

    def get_busy(self) -> bool:
        return self._busy

    def _set_busy(self, val: bool) -> None:
        if val != self._busy:
            self._busy = val
            self.busyChanged.emit(val)

    busy = Property(bool, get_busy, notify=busyChanged)

    loggedInChanged = Signal(bool)

    def get_logged_in(self) -> bool:
        return self._logged_in

    def _set_logged_in(self, new_val: bool) -> None:
        if new_val != self._logged_in:
            self._logged_in = new_val
            self.loggedInChanged.emit(new_val)

    loggedIn = Property(bool, get_logged_in, notify=loggedInChanged)

    @Slot()
    def doLogin(self) -> None:
        print(f"logging in with {self.get_username()} / {self.get_password()}...")
        # `DoLogin` asynchronously calls back into model methods to update the state
        # based on the result of the login call:
        self.threadpool.start(DoLogin(self))

    @Slot()
    def doLogout(self) -> None:
        # clear stored credentials:
        self.set_username("")
        self.set_password("")
        self._set_logged_in(False)


class PageStateModel(QObject):
    def __init__(self, login_model: LoginModel) -> None:
        self._login_model = login_model
        self._current_page = "login.qml"
        super().__init__()

    currentPageChanged = Signal(str)

    def get_current_page(self) -> str:
        return self._current_page

    def _set_current_page(self, page: str):
        if page != self._current_page:
            self._current_page = page
            self.currentPageChanged.emit(page)

    currentPage = Property(str, get_current_page, notify=currentPageChanged)

    @Slot()
    def loginStateChanged(self) -> None:
        """
        Logged in state changed (either logged in or logged out)
        """
        if self._login_model.loggedIn:
            self._set_current_page("overview.qml")
        else:
            self._set_current_page("login.qml")

    @Slot()
    def startSessionDialog(self) -> None:
        # pre-conditions: need to be logged in
        assert self._login_model.loggedIn
        self._set_current_page("new-session.qml")

    @Slot()
    def _session_start(self) -> None:
        self._set_current_page("overview.qml")


class ProjectListModel(QAbstractListModel):
    def __init__(self, login_model: LoginModel) -> None:
        super().__init__()
        self._projects: List[str] = []
        self.threadpool = QThreadPool()
        self._login_model = login_model
        self._fetch()

    def _set_projects(self, projects: List[str]) -> None:
        self._projects = projects
        self.dataChanged.emit(self.index(0, 0), self.index(len(projects) - 1, 0))
        print(f"setting projects to {projects}")

    def _fetch(self) -> None:
        print("fetching list of projects...")
        assert self._login_model is not None
        self.threadpool.start(DoListProjects(self, self._login_model))

    def rowCount(self, parent) -> int:
        print(f"rowCount() -> {len(self._projects)}")
        return len(self._projects)

    def get_default_project(self):
        if len(self._projects) == 0:
            return None
        return self._projects[0]

    def data(self, index, role):
        print(f"data({index}, {role})")
        if (role == Qt.DisplayRole
                and index.row() >= 0
                and index.row() < len(self._projects)
                and index.column() == 0):
            return self._projects[index.row()]
        else:
            return None


class ListOfDictModel(QAbstractListModel):
    def __init__(self, list_of_roles: List[str]) -> None:
        super().__init__()
        self.schema = list_of_roles
        self.items: List[Dict] = []

    def data(self, index, role):
        key = self.schema[role]
        value = self.items[index.row()].get(key)
        print(f"data({index}, {key}) = {value}")
        return value

    def replace_data(self, new_items: List[Dict]) -> None:
        """
        Replace all entries of this model at once

        Parameters
        ----------
        new_items : List[Dict]
            The new list items; the dicts must have the keys specified in
            `list_of_roles`
        """
        if new_items != self.items:
            self.items = new_items
            print(f"replacing LoD data with {new_items}")
            self.dataChanged.emit(self.index(0, 0), self.index(len(new_items) - 1, 0))

    def rowCount(self, parent) -> int:
        return len(self.items)

    def roleNames(self) -> Dict[int, str]:
        return dict(enumerate(self.schema))


class SessionModel(QObject):
    def __init__(self, login_model: "LoginModel", projects_model: ProjectListModel, pagestate_model: PageStateModel):
        self._login_model = login_model
        self._projects_model = projects_model
        self._pagestate_model = pagestate_model
        self._shares = ListOfDictModel([
            "created_timestamp", "dest_path", "gid", "password",
            "share_id", "share_name", "uid", "username",
        ])
        self._session_share_id: Optional[str] = None
        self._session_name: Optional[str] = None
        self._project_name: Optional[str] = None
        self._busy: bool = False

        self.threadpool = QThreadPool()
        super().__init__()
        self._fetch_sessions()

        # connect some signals:
        self._projects_model.dataChanged.connect(self._set_default_project)
        self._login_model.loggedInChanged.connect(self._projects_model._fetch)
        self.sessionStart.connect(self._pagestate_model._session_start)

    def _set_default_project(self):
        if self._project_name is None:
            if self._projects_model.rowCount(None) > 0:
                self.set_project_name(self._projects_model.get_default_project())

    def _fetch_sessions(self) -> None:
        self._set_busy(True)
        self.threadpool.start(DoListSessions(self, self._login_model))

    def _set_shares(self, shares: List[Dict]) -> None:
        self._shares.replace_data(shares)

    @Slot()
    def get_shares(self) -> ListOfDictModel:
        return self._shares

    sessionShareChanged = Signal(str)

    def _set_session_share_id(self, share_id: str) -> None:
        if share_id != self._session_share_id:
            self._session_share_id = share_id
            self.sessionShareChanged.emit(share_id)

    sessionNameChanged = Signal(str)

    def get_session_name(self) -> Optional[str]:
        return self._session_name

    def set_session_name(self, session_name: str) -> None:
        if session_name != self._session_name:
            self._session_name = session_name
            self.sessionNameChanged.emit(session_name)

    sessionName = Property(str, get_session_name, set_session_name, notify=sessionNameChanged)

    projectNameChanged = Signal(str)

    def get_project_name(self) -> Optional[str]:
        return self._project_name

    def set_project_name(self, project_name: str) -> None:
        if project_name != self._project_name:
            self._project_name = project_name
            self.projectNameChanged.emit(project_name)

    projectName = Property(str, get_project_name, set_project_name, notify=projectNameChanged)

    sessionStart = Signal()
    sessionStop = Signal()

    @Slot()
    def startSession(self) -> None:
        print(f"starting session {self._session_name}")
        self._set_busy(True)
        self.threadpool.start(DoCreateSession(
            self,
            self._login_model,
            project_name=self.projectName,
            session_name=self.sessionName,
        ))

    busyChanged = Signal(bool)

    def get_busy(self) -> bool:
        return self._busy

    def _set_busy(self, busy: bool) -> None:
        if busy != self._busy:
            self._busy = busy
            self.busyChanged.emit(busy)

    busy = Property(bool, get_busy, _set_busy, notify=busyChanged)


if __name__ == "__main__":
    QQuickStyle.setStyle("Material")

    # "fix" CTRL+C handling:
    signal.signal(signal.SIGINT, signal.SIG_DFL)

    app = QGuiApplication(sys.argv)
    engine = QQmlApplicationEngine()

    # instantiate and register global objects:
    login_model = LoginModel()
    pagestate_model = PageStateModel(login_model)
    projects_model = ProjectListModel(login_model)
    session_model = SessionModel(login_model, projects_model, pagestate_model)
    engine.rootContext().setContextProperty("loginModel", login_model)
    engine.rootContext().setContextProperty("pageStateModel", pagestate_model)
    engine.rootContext().setContextProperty("projectListModel", projects_model)
    engine.rootContext().setContextProperty("sessionModel", session_model)

    engine.load(os.path.join(os.path.dirname(__file__), "main.qml"))

    if not engine.rootObjects():
        sys.exit(-1)
    sys.exit(app.exec())
