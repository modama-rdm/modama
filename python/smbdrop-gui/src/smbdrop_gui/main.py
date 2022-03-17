from typing import Any, Dict, List, Optional
from datetime import datetime
from pathlib import Path
import signal
import time
import copy
import sys
import os

import requests
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlApplicationEngine, qmlRegisterType
from PySide6.QtQuickControls2 import QQuickStyle
from PySide6.QtCore import (
    Qt, QObject, Signal, Slot, Property, QRunnable, QThreadPool,
    QAbstractListModel, QTimer,
)

from smbdrop_client import ShareClient
from smbdrop_gui.mount import get_mm_class, MountManager


PERMISSIONS_API_BASE_URL = "http://172.28.0.2:5050"
SMBDROP_API_BASE_URL = "http://127.0.0.1:5051"


class DoLogin(QRunnable):
    def __init__(self, model: "LoginModel") -> None:
        super().__init__()
        self.model = model

    @Slot()
    def run(self) -> None:
        self.model._set_busy(True)
        try:
            time.sleep(0.1)  # simulate network...
            print("login success!")
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


class DoCloseSession(QRunnable):
    def __init__(self, model: "SessionModel", login_model: "LoginModel", share_id: str) -> None:
        super().__init__()
        self.model = model
        self.login_model = login_model
        self.share_id = share_id

    @Slot()
    def run(self) -> None:
        # FIXME: error handling!
        try:
            c = ShareClient(SMBDROP_API_BASE_URL)
            c.delete(self.share_id)
            shares = c.list()
            self.model._set_shares(list(shares.values()))
            self.model._set_session_share_id(None)
        finally:
            self.model._set_busy(False)


class DoUpdateSharesList(QRunnable):
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
            self.model._set_shares(list(shares.values()))
        finally:
            # as this is run in the background, we don't set/unset busy
            # self.model._set_busy(False)
            pass


class DoMapDrive(QRunnable):
    def __init__(self, model: "SessionModel", share_id: str, mm: MountManager) -> None:
        super().__init__()
        self.model = model
        self.share_id = share_id
        self.mm = mm

    @Slot()
    def run(self) -> None:
        self.model._set_busy(True)
        try:
            print(f"really mapping network drive {self.share_id}...")
            share_info = self.model.get_share_by_id(self.share_id)
            mm = self.mm
            mm.mount_smb(
                share_name=share_info["share_name"],
                username=share_info["username"],
                password=share_info["password"],
            )
            mm.open(
                share_name=share_info["share_name"],
                username=share_info["username"],
            )
            print(share_info)
        finally:
            self.model._set_busy(False)


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
    def cancelToOverview(self) -> None:
        self._set_current_page("overview.qml")

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
        return len(self._projects)

    def get_default_project(self) -> Optional[str]:
        if len(self._projects) == 0:
            return None
        return self._projects[0]

    def data(self, index, role):
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
        self._schema = list_of_roles
        self._items: List[Dict] = []

    def data(self, index, role):
        key = self._schema[role]
        value = self._items[index.row()].get(key)
        return value

    def get_all(self):
        return self._items

    def replace_data(self, new_items: List[Dict]) -> None:
        """
        Replace all entries of this model at once

        Parameters
        ----------
        new_items : List[Dict]
            The new list items; the dicts must have the keys specified in
            `list_of_roles`
        """
        if new_items != self._items:
            old_length = len(self._items)
            if len(new_items) != old_length:
                # different length, just reset the model:
                self.beginResetModel()
                self._items = new_items
                self.endResetModel()
            else:
                # same length, just notify that things have changed:
                self._items = new_items
                # make sure, if we removed items, that we notify for the full range,
                # not only the new (possibly smaller) index range:
                self.dataChanged.emit(self.index(0, 0), self.index(len(new_items) - 1, 0))

    def rowCount(self, __parent) -> int:
        return len(self._items)

    def roleNames(self) -> Dict[int, str]:
        # NOTE: Qt really doesn't like `str`-keyed dicts for `roleNames` for
        # some reason...
        roles = dict(enumerate(
            [s.encode("utf8") for s in self._schema]
        ))
        return roles


class SessionModel(QObject):
    def __init__(
        self,
        login_model: "LoginModel",
        projects_model: ProjectListModel,
        pagestate_model: PageStateModel,
        shares_model: ListOfDictModel,
        mm: MountManager,
    ):
        self._login_model = login_model
        self._projects_model = projects_model
        self._pagestate_model = pagestate_model
        self._shares = shares_model
        self._session_share_id: Optional[str] = None
        self._session_name: Optional[str] = None
        self._project_name: Optional[str] = None
        self._busy: bool = False
        self._mm = mm

        self.threadpool = QThreadPool()
        super().__init__()
        self._fetch_sessions()

        # connect some signals:
        self._projects_model.dataChanged.connect(self._set_default_project)
        self._login_model.loggedInChanged.connect(self._projects_model._fetch)
        self.sessionStart.connect(self._pagestate_model._session_start)

        # regular updates in the background:
        self._update_timer = QTimer(self)
        self._update_timer.setInterval(500)
        self._update_timer.timeout.connect(self._update_bg)
        self._update_timer.start()

        self._update_bg()

    def _set_default_project(self) -> None:
        if self._project_name is None:
            if self._projects_model.rowCount(None) > 0:
                self.set_project_name(self._projects_model.get_default_project())

    def _fetch_sessions(self) -> None:
        self._set_busy(True)
        self.threadpool.start(DoListSessions(self, self._login_model))

    def _preprocess_share(self, share: Dict) -> Dict:
        share = copy.copy(share)
        created = datetime.fromtimestamp(share["created_timestamp"])
        share["created_datetime"] = created.strftime("%c")
        dest_path = Path(share["dest_path"])
        trunc_path = "/".join(dest_path.parts[-2:])
        share["title"] = f"{trunc_path}"
        return share

    def _set_shares(self, shares: List[Dict]) -> None:
        # pre-process and set shares:
        new_shares = [
            self._preprocess_share(s)
            for s in shares
        ]
        self._shares.replace_data(new_shares)

    def _update_bg(self) -> None:
        if not self._busy and self._login_model.loggedIn:
            # print("updating...")
            self.threadpool.start(DoUpdateSharesList(
                model=self,
                login_model=self._login_model,
            ))

    # FIXME: probably needs a result='' Qt-specific type annotation here!
    # like this, it just doesn't work...
    @Slot()
    def get_shares(self) -> ListOfDictModel:
        return self._shares

    sessionShareChanged = Signal(str)

    def _set_session_share_id(self, share_id: Optional[str]) -> None:
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

    @Slot(str)
    def closeSession(self, sessionId: str) -> None:
        print(f"closing session {sessionId}")
        self.threadpool.start(
            DoCloseSession(self, self._login_model, sessionId)
        )

    def get_share_by_id(self, share_id: str) -> Dict:
        # linear search, stupid but... works?
        # TODO: might want to extend the ListOfDictModel by a
        # "primary key" concept, by which we can then index stuff
        # (in addition to just listing...)
        for share in self._shares.get_all():
            if share['share_id'] == share_id:
                return share
        raise RuntimeError(f"invalid share id: {share_id}")

    @Slot(str)
    def mapSessionDrive(self, sessionId: str) -> None:
        print(f"mapping network drive for session id {sessionId}")
        self.threadpool.start(
            DoMapDrive(self, sessionId, self._mm)
        )

    busyChanged = Signal(bool)

    def get_busy(self) -> bool:
        return self._busy

    def _set_busy(self, busy: bool) -> None:
        if busy != self._busy:
            self._busy = busy
            self.busyChanged.emit(busy)

    busy = Property(bool, get_busy, _set_busy, notify=busyChanged)


def main():
    QQuickStyle.setStyle("Material")

    # "fix" CTRL+C handling:
    signal.signal(signal.SIGINT, signal.SIG_DFL)

    app = QGuiApplication(sys.argv)
    engine = QQmlApplicationEngine()

    MountManagerClass = get_mm_class()
    mm = MountManagerClass(smb_host="localhost", workgroup="ER-C-INGEST")

    # instantiate and register global objects:
    login_model = LoginModel()
    pagestate_model = PageStateModel(login_model)
    projects_model = ProjectListModel(login_model)
    shares_model = ListOfDictModel([
        "created_timestamp", "dest_path", "gid", "password",
        "share_id", "share_name", "uid", "username",

        # derived data:
        "title", "created_datetime",
    ])
    session_model = SessionModel(
        login_model, projects_model, pagestate_model, shares_model, mm
    )
    engine.rootContext().setContextProperty("loginModel", login_model)
    engine.rootContext().setContextProperty("pageStateModel", pagestate_model)
    engine.rootContext().setContextProperty("projectListModel", projects_model)
    engine.rootContext().setContextProperty("sessionModel", session_model)
    engine.rootContext().setContextProperty("sharesModel", shares_model)

    engine.load(os.path.join(os.path.dirname(__file__), "main.qml"))

    if not engine.rootObjects():
        sys.exit(-1)
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
