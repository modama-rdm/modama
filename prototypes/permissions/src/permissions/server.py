from typing import Dict, TypedDict, List
from flask import Flask, request, jsonify
import jsonschema

from permissions.api import AdhocProject, ValidationError

app = Flask(__name__)


class InvalidAPIUsage(Exception):
    """
    Exception type for conveying the right status code to return
    """
    status_code = 400

    def __init__(self, message, status_code=None, payload=None):
        super().__init__()
        self.message = message
        if status_code is not None:
            self.status_code = status_code
        self.payload = payload

    def to_dict(self):
        rv = dict(self.payload or ())
        rv['message'] = self.message
        return rv


@app.errorhandler(InvalidAPIUsage)
def invalid_api_usage(e):
    return jsonify(e.to_dict()), e.status_code


CREATE_PROJECT_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to create a new project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Short name of the project",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the project",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 1,
            "uniqueItems": True,
            "description": "Usernames of the members of this ad-hoc group",
        },
    },
    "required": ["name", "owner", "members"],
}


class CreateProjectRequest(TypedDict):
    name: str
    owner: str
    members: List[str]


@app.route("/project/", methods=["POST"])
def create_project() -> Dict:
    request_data: CreateProjectRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=CREATE_PROJECT_REQUEST_SCHEMA, instance=request_data
        )
        project = AdhocProject.create(
            name=request_data['name'],
            owner=request_data['owner'],
            members=request_data['members']
        )
        return {
            'project': project.serialize(),
        }
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/project/", methods=["GET"])
def list_projects() -> Dict:
    # TODO: catch exceptions? OSError?
    project_names = AdhocProject.list_all()
    return {
        'projects': [
            AdhocProject(project_name).serialize(fast=True)
            for project_name in project_names
        ]
    }


@app.route("/project/<project_name>/", methods=["GET"])
def project_details(project_name) -> Dict:
    # TODO: catch exceptions? can be various:
    # - from calling `du` (subprocess specific errors?)
    # - from posix1e.ACL(filedef=...)
    # - from pwd.getpwuid
    # - ValidationError from the constructor
    try:
        project = AdhocProject(project_name)
        return {
            'project': project.serialize(),
        }
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))


@app.route("/project/<project_name>/dataset/", methods=["GET"])
def list_datasets(project_name) -> Dict:
    try:
        p = AdhocProject(project_name)
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))

    # TODO: catch exceptions? `p.datasets` can raise multiple...
    return {
        "project": project_name,
        "datasets": [
            {"name": ds.name, "immutable": ds.immutable}
            for ds in p.datasets
        ]
    }


CREATE_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-dataset-request.schema.json",
    "title": "CreateDatasetRequest",
    "description": "The JSON payload for the POST request to create a new dataset in a project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset, which will be used as directory name",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the dataset",
        },
    },
    "required": ["name", "owner"],
}


class CreateDatasetRequest(TypedDict):
    name: str
    owner: str


@app.route("/project/<project_name>/dataset/", methods=["POST"])
def create_dataset(project_name) -> Dict:
    request_data: CreateDatasetRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=CREATE_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        p = AdhocProject(project_name)
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    # TODO: catch exceptions? for example, `CalledProcessError` if `mkdir` fails...
    p.add_dataset(
        name=request_data["name"],
        user=request_data["owner"]
    )
    return {}  # what should be returned here?


SEAL_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/seal-dataset-request.schema.json",
    "title": "SealDatasetRequest",
    "description": "The JSON payload for the POST request to seal a dataset",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset",
        },
    },
    "required": ["name"],
}


class SealDatasetRequest(TypedDict):
    name: str


# NOTE: not using data set name in path, as it can contain slashes (right?) so
# we are using a json payload for this request, which may seem a bit like
# overkill....
@app.route("/project/<project_name>/dataset/seal/", methods=["POST"])
def seal_dataset(project_name):
    request_data: CreateDatasetRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=SEAL_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        # TODO: catch exceptions?
        p = AdhocProject(project_name)
        p.seal_dataset(request_data["name"])
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    return {}  # what should be returned here?


CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to change membership of a project",
    "type": "object",
    "properties": {
        "op": {
            "type": "string",
            "enum": ["add", "remove"],
            "description": "Operation: add or remove",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 1,
            "uniqueItems": True,
            "description": "Usernames of the members to add or remove from this group",
        },
    },
    "required": ["op", "members"],
}


class ChangeMembersRequest(TypedDict):
    op: str
    members: List[str]


@app.route("/project/<project_name>/members/", methods=["PATCH"])
def change_project_members(project_name):
    request_data: ChangeMembersRequest = request.json  # type:ignore
    jsonschema.validate(
        schema=CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA, instance=request_data
    )
    # TODO: catch exceptions from `add_members` and `remove_members`
    try:
        p = AdhocProject(project_name)
        op = request_data["op"]
        if op == "add":
            p.add_members(request_data["members"])
        elif op == "remove":
            p.remove_members(request_data["members"])
        else:
            raise InvalidAPIUsage(f"Unknown op: {op}")
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    return {
        "project": p.serialize()
    }
